import styles from "./settings.module.css";

const external = { target: "_blank", rel: "noreferrer noopener" } as const;

/** The Discord application, gateway and channel permissions needed by the bot. */
export function DiscordSetupHelp() {
  return (
    <div className={styles.setupHelp}>
      <p>
        Create an application and bot in the{" "}
        <a href="https://discord.com/developers/applications" {...external}>
          Discord developer portal
        </a>
        . Under <strong>Bot</strong>, copy the token and enable{" "}
        <strong>Message Content Intent</strong> so the bot can read questions in incident threads.
      </p>
      <p>
        Under <strong>OAuth2 → URL Generator</strong>, select the <code>bot</code> and{" "}
        <code>applications.commands</code> scopes; the latter enables slash commands. Grant View
        Channels, Send Messages, Embed Links, Read Message History, Create Public Threads and Send
        Messages in Threads, then invite the bot. See Discord&apos;s{" "}
        <a href="https://discord.com/developers/docs/topics/permissions" {...external}>
          permissions documentation
        </a>
        .
      </p>
      <p>
        Enable <strong>Developer Mode</strong> in Discord under{" "}
        <strong>User Settings → Advanced</strong> to copy numeric channel and user IDs.
      </p>
    </div>
  );
}
