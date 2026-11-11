import { Section, Stack } from "../../components";
import { AddWebhook } from "./AddWebhook";
import { DiscordSetupHelp } from "./DiscordSetupHelp";
import { SectionFields, type FieldsContext } from "./SectionFields";
import { SectionTest } from "./SectionTest";
import type { SectionSpec } from "./sections";

/**
 * One section of the settings: its title and sentence, a slot for a status line (a section
 * that has one draws it here, above the fields), its fields and its connection test.
 * A section with a `view` draws that instead of fields.
 */
export function SectionBody({
  section,
  context,
}: {
  section: SectionSpec;
  context: FieldsContext;
}) {
  const View = section.view;
  const fields = context.form.visible(section.id);
  return (
    <Section title={section.title} description={section.intro}>
      <Stack gap={5}>
        {View && <View />}
        {section.extra === "discord-help" && <DiscordSetupHelp />}
        {section.extra === "add-webhook" && <AddWebhook onAdd={context.form.addWebhook} />}
        {fields.length > 0 && <SectionFields fields={fields} context={context} />}
        {section.test && (
          <SectionTest
            target={section.test}
            slot={section.testSlot}
            changes={context.form.changes}
          />
        )}
      </Stack>
    </Section>
  );
}
