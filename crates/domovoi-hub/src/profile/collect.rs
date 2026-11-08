//! Asks one host's agent what its machine looks like while it behaves normally.
//!
//! Every request is a read tool from the closed [`crate::domovoi_core::ReadToolCall`]
//! enum, so collecting a profile cannot change the machine it describes. A tool that
//! fails, times out or is refused becomes a section saying so: one missing part does
//! not throw away a profile, but the gap has to be visible in it.

use std::collections::BTreeMap;

use domovoi_core::ports::Presence;
use domovoi_core::{Host, HostId, ReadToolCall, Risk, SkillSummary};
use domovoi_proto::messages::{CheckInfo, HostFacts};
use jiff::SignedDuration;

use super::ContainerLine;
use super::HostProfile;
use super::ProfileDeps;
use super::ProfileError;
use super::ProfileFacts;
use super::ProfileSection;
use super::cut;

/// How long one read tool may take before the link gives up on it.
pub(crate) const TOOL_DEADLINE: SignedDuration = SignedDuration::from_secs(20);

/// A section longer than this is cut.
pub(crate) const SECTION_LIMIT: usize = 8 * 1024;

/// The line left where the agent's own cap cut the output.
pub(crate) const AGENT_CUT: &str = "\n[the agent cut this output]";

/// Section titles for the tools the hub always asks.
pub(crate) const OVERVIEW_TITLE: &str = "Overview";
pub(crate) const CONTAINER_TITLE: &str = "Containers";
pub(crate) const LISTENER_TITLE: &str = "Listening ports";
pub(crate) const KERNEL_TITLE: &str = "Kernel messages (24 h)";

/// What the hub reads back from the journal: the last day of kernel messages.
pub(crate) const JOURNAL_SINCE_MINUTES: u32 = 1440;
pub(crate) const JOURNAL_LINES: u16 = 100;

/// Read skills one profile asks, so a host with a large skill set stays cheap.
pub(crate) const MAX_SKILL_SECTIONS: usize = 12;

/// Asks the agent for one thing and returns its text, or why it did not answer.
async fn ask(deps: &ProfileDeps, host: HostId, call: ReadToolCall) -> Result<String, String> {
    let now = deps.clock.now();
    let deadline = now.checked_add(TOOL_DEADLINE).unwrap_or(now);
    let out = deps
        .link
        .run_tool(host, call, deadline)
        .await
        .map_err(|err| err.to_string())?;
    let text = deps.redactor.redact(&out.text);
    if out.truncated {
        return Ok(text + AGENT_CUT);
    }
    Ok(text)
}

/// A profile section from what a tool answered, or from why it did not.
pub(crate) fn section_of(title: &str, answered: Result<String, String>) -> ProfileSection {
    match answered {
        Ok(text) => ProfileSection {
            title: title.to_owned(),
            text: cut(text, SECTION_LIMIT),
            ok: true,
        },
        Err(why) => ProfileSection {
            title: title.to_owned(),
            text: why,
            ok: false,
        },
    }
}

/// One container from one `docker ps` line, or `None` for the header and short lines.
pub(crate) fn container_of(line: &str) -> Option<ContainerLine> {
    let fields: Vec<&str> = line.split('\t').collect();
    if fields.len() < 5 {
        return None;
    }
    Some(ContainerLine {
        name: fields[0].to_owned(),
        image: fields[1].to_owned(),
        state: fields[2].to_owned(),
        health: fields[3].to_owned(),
        restarts: fields[4].to_owned(),
    })
}

/// The containers the agent's `docker ps` text listed; its first line is a header.
pub(crate) fn containers_of(text: &str) -> Vec<ContainerLine> {
    text.lines()
        .skip(1)
        .filter_map(|line| container_of(line.trim()))
        .collect()
}

/// The sockets answered outside loopback, one per line.
pub(crate) fn listeners_of(text: &str) -> Vec<String> {
    text.lines()
        .map(|line| line.trim().to_owned())
        .filter(|line| !line.is_empty())
        .filter(|line| line != "no listeners outside loopback")
        .collect()
}

/// The machine facts to keep, from the ones the agent shook hands with.
pub(crate) fn facts_of(facts: HostFacts) -> ProfileFacts {
    ProfileFacts {
        hostname: facts.hostname,
        os: facts.os,
        kernel: facts.kernel,
        arch: facts.arch,
        cpu_count: facts.cpu_count,
        mem_total_bytes: facts.mem_total_bytes,
        boot_time: facts.boot_time,
    }
}

/// The checks the agent's own config leaves on, as one line each.
pub(crate) fn checks_of(checks: Vec<CheckInfo>) -> Vec<String> {
    checks
        .iter()
        .map(|check| {
            let state = if check.enabled { "on" } else { "off" };
            format!(
                "{}: every {} s {}",
                check.key.as_str(),
                check.interval_seconds,
                state
            )
        })
        .collect()
}

/// The skills the agent announced, as `name (risk)`.
pub(crate) fn skills_of(summaries: Vec<SkillSummary>) -> Vec<String> {
    summaries
        .iter()
        .map(|skill| format!("{} ({:?})", skill.name.as_str(), skill.risk))
        .collect()
}

/// Collects the profile of `host` by asking its agent.
///
/// Fails only when the host has no connected agent to ask; a tool that fails becomes a
/// section saying so.
pub(crate) async fn collect(deps: &ProfileDeps, host: &Host) -> Result<HostProfile, ProfileError> {
    let connected = deps.link.presence(host.id);
    let version = match connected {
        Presence::Connected { version, .. } => version,
        Presence::Disconnected { .. } => return Err(ProfileError::NotConnected),
    };
    let handshake = deps
        .agents
        .facts(host.id)
        .ok_or(ProfileError::NotConnected)?;
    let overview = ask(deps, host.id, ReadToolCall::HostOverview).await;
    let docker = ask(deps, host.id, ReadToolCall::DockerPs { all: true }).await;
    let ports = ask(deps, host.id, ReadToolCall::ListeningPorts).await;
    let kernel = ask(
        deps,
        host.id,
        ReadToolCall::Journal {
            unit: None,
            since_minutes: JOURNAL_SINCE_MINUTES,
            lines: JOURNAL_LINES,
            kernel: true,
        },
    )
    .await;
    let overview_section = section_of(OVERVIEW_TITLE, overview);
    let docker_section = section_of(CONTAINER_TITLE, docker);
    let ports_section = section_of(LISTENER_TITLE, ports);
    let kernel_section = section_of(KERNEL_TITLE, kernel);
    let containers = match docker_section.ok {
        true => containers_of(&docker_section.text),
        false => [].to_vec(),
    };
    let listeners = match ports_section.ok {
        true => listeners_of(&ports_section.text),
        false => [].to_vec(),
    };
    let mut sections = [
        overview_section,
        docker_section,
        ports_section,
        kernel_section,
    ]
    .to_vec();
    let summaries = deps.link.skills(host.id);
    for skill in summaries
        .iter()
        .filter(|skill| skill.risk == Risk::Read)
        .take(MAX_SKILL_SECTIONS)
    {
        let call = ReadToolCall::RunReadSkill {
            skill: skill.name.clone(),
            args: BTreeMap::new(),
        };
        let answered = ask(deps, host.id, call).await;
        sections.push(section_of(skill.name.as_str(), answered));
    }
    Ok(HostProfile {
        collected_at: deps.clock.now(),
        agent_version: version,
        facts: facts_of(handshake),
        containers,
        listeners,
        skills: skills_of(summaries),
        checks: checks_of(deps.agents.checks(host.id)),
        sections,
        note: None,
    })
}
