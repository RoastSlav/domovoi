//! Where a skill is found: the owner's skills directory first, then the skills of enabled
//! packages. A stored package that nobody enabled has no runnable skill here.

use std::borrow::Cow;

use domovoi_core::ValidatedArgs;
use domovoi_core::ports::SkillOutput;
use domovoi_core::skill::SkillName;
use domovoi_core::skill_runner::SkillSource;
use domovoi_runner::{LoadedSkill, SkillCatalog};

use super::RequestCtx;
use crate::packages::enable::SkillView;
use crate::skill_runner::RouteError;

/// A skill and the place it was found in.
pub struct Found<'a> {
    /// The skill.
    pub skill: Cow<'a, LoadedSkill>,
    /// The catalogue the route checks it against.
    pub catalog: Cow<'a, SkillCatalog>,
    /// Where the runner finds its folder.
    pub source: SkillSource,
}

impl RequestCtx {
    /// The skills of the stored packages, on or off, and the ones that may run now. Every
    /// call re-checks the files and the signer.
    #[must_use]
    pub fn package_skills(&self) -> SkillView {
        let Some(service) = &self.packages else {
            return SkillView::default();
        };
        let owner: Vec<String> = self
            .catalog
            .summaries()
            .iter()
            .map(|s| s.name.to_string())
            .collect();
        service.skill_view(&owner)
    }

    /// Names of the package skills that may run now.
    #[must_use]
    pub fn enabled_package_skill_names(&self) -> Vec<SkillName> {
        self.package_skills()
            .runnable
            .iter()
            .map(|r| r.skill.manifest().name.clone())
            .collect()
    }

    /// The package surface (same as `package_surface`, named for the catalogue call).
    #[must_use]
    pub fn package_skills_surface(&self) -> crate::packages::PackageSurface {
        self.package_surface()
    }

    /// The skill `name`: an owner skill first, else an enabled package skill.
    #[must_use]
    pub fn find_skill(&self, name: &SkillName) -> Option<Found<'_>> {
        if let Some(skill) = self.catalog.get(name) {
            return Some(Found {
                skill: Cow::Borrowed(skill),
                catalog: Cow::Borrowed(&*self.catalog),
                source: SkillSource::SkillsDir,
            });
        }
        let runnable = self
            .package_skills()
            .runnable
            .into_iter()
            .find(|r| &r.skill.manifest().name == name)?;
        Some(Found {
            skill: Cow::Owned(runnable.skill),
            catalog: Cow::Owned(runnable.catalog),
            source: runnable.source,
        })
    }

    /// Runs a found skill through the route, from the source it was found in.
    ///
    /// # Errors
    /// The route's refusal or failure.
    pub async fn run_found(
        &self,
        found: &Found<'_>,
        args: &ValidatedArgs,
    ) -> Result<SkillOutput, RouteError> {
        self.skills
            .run_from(&found.skill, &found.catalog, &found.source, args)
            .await
    }
}
