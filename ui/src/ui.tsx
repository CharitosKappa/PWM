/** Small shared pieces of the popup. */
import type { ComponentChildren, JSX } from "preact";

interface FieldProps {
  label: string;
  value: string;
  onInput: (value: string) => void;
  type?: "text" | "email" | "password" | "url";
  autoComplete?: string;
  mono?: boolean;
  hint?: string;
}

export function Field({
  label,
  value,
  onInput,
  type = "text",
  autoComplete = "off",
  mono = false,
  hint,
}: FieldProps): JSX.Element {
  return (
    <label class="field">
      <span class="label">{label}</span>
      <input
        class={mono ? "mono" : undefined}
        type={type}
        value={value}
        autoComplete={autoComplete}
        spellcheck={false}
        onInput={(event) => {
          onInput(event.currentTarget.value);
        }}
      />
      {hint !== undefined && <span class="caption muted">{hint}</span>}
    </label>
  );
}

export function Notice({
  kind,
  children,
}: {
  kind: "danger" | "warning" | "info";
  children: ComponentChildren;
}): JSX.Element {
  return (
    <p class={`notice ${kind}`} role={kind === "danger" ? "alert" : "status"}>
      {children}
    </p>
  );
}
