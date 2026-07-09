// Closed vocabulary of omw LLM provider kinds (the `kind = ...` discriminator).
// Single source of truth: specs/provider-kinds.txt (a golden test asserts
// parity). Rust canonical: crates/omw-config (ProviderConfig::kind_str +
// omw_config::PROVIDER_KINDS).

export const PROVIDER_KINDS = [
  "anthropic",
  "ollama",
  "openai",
  "openai-compatible",
] as const;

export type ProviderKind = (typeof PROVIDER_KINDS)[number];

/** Narrowing guard: true iff `x` is a known provider kind. */
export function isProviderKind(x: unknown): x is ProviderKind {
  return typeof x === "string" && (PROVIDER_KINDS as readonly string[]).includes(x);
}
