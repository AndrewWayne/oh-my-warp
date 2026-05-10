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
    expect(textarea.spellcheck).toBe(false);
  });
});
