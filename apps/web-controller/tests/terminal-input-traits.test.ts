import { describe, expect, it } from "vitest";
import { configureTerminalInputTraits } from "../src/lib/terminal-input-traits";

describe("configureTerminalInputTraits", () => {
  it("disables mobile text assistance on xterm's hidden textarea", () => {
    const root = document.createElement("div");
    const textarea = document.createElement("textarea");
    root.append(textarea);

    configureTerminalInputTraits(root);

    expect(textarea).toHaveAttribute("autocomplete", "off");
    expect(textarea).toHaveAttribute("autocorrect", "off");
    expect(textarea).toHaveAttribute("autocapitalize", "none");
    expect(textarea).toHaveAttribute("spellcheck", "false");
    expect(textarea).toHaveAttribute("enterkeyhint", "enter");
    expect(textarea).toHaveAttribute("inputmode", "search");
    expect(textarea).toHaveAttribute("aria-autocomplete", "none");
    expect(textarea).toHaveAttribute("name", "omw-terminal-input");
    expect(textarea).toHaveAttribute("data-terminal-input", "true");
    expect(textarea).toHaveAttribute("autofill", "off");
    expect(textarea).toHaveAttribute("autofill-prediction", "off");
    expect(textarea).toHaveAttribute("data-form-type", "other");
    expect(textarea).toHaveAttribute("data-lpignore", "true");
    expect(textarea).toHaveAttribute("data-1p-ignore", "true");
    expect(textarea).toHaveAttribute("data-bwignore", "true");
    expect(textarea.spellcheck).toBe(false);
  });
});
