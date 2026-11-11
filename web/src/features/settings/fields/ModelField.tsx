import { useId } from "react";
import { useModelDiscovery, type Changes, type ProviderSlot } from "../../../api/hooks/settings";
import {
  Button,
  Cluster,
  Field as KitField,
  InlineError,
  Stack,
  Text,
  TextInput,
} from "../../../components";
import { errorText, type Common } from "./common";
import { SourceTag } from "./SourceTag";

function modelsLine(count: number): string {
  if (count === 0) {
    return "The provider returned no models. Enter a model ID manually.";
  }
  return `${count} ${count === 1 ? "model" : "models"} available. Choose one or enter another model ID.`;
}

/**
 * A model ID with provider-backed suggestions and a manual-entry fallback; `slot` picks
 * which provider lists them, the main one by default.
 */
export function ModelField({
  spec,
  value,
  onChange,
  candidate,
  slot,
  hint,
  errors,
  locked,
  source,
}: Common & {
  value: string;
  onChange: (value: string) => void;
  candidate: () => Changes;
  slot?: ProviderSlot | undefined;
}) {
  const listId = useId();
  const discovery = useModelDiscovery(slot);
  const models = discovery.data ?? [];
  return (
    <KitField
      label={spec.label}
      path={spec.path}
      hint={hint}
      error={errorText(errors)}
      locked={locked}
      badges={source ? <SourceTag source={source} /> : undefined}
    >
      <Stack gap={2}>
        <TextInput
          list={`${listId}-models`}
          value={value}
          placeholder={spec.placeholder}
          autoComplete="off"
          onChange={(event) => onChange(event.target.value)}
        />
        <datalist id={`${listId}-models`}>
          {models.map((model) => (
            <option key={model} value={model} />
          ))}
        </datalist>
        {!locked && (
          <Cluster>
            <Button loading={discovery.isPending} onClick={() => discovery.mutate(candidate())}>
              Fetch models
            </Button>
            {discovery.data && (
              <Text muted size="sm">
                {modelsLine(models.length)}
              </Text>
            )}
          </Cluster>
        )}
        <InlineError error={discovery.error} context="fetch the models" />
      </Stack>
    </KitField>
  );
}
