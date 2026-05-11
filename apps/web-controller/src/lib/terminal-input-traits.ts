export function configureTerminalInputTraits(root: HTMLElement): void {
  const textarea = root.querySelector("textarea");
  if (!(textarea instanceof HTMLTextAreaElement)) return;

  textarea.setAttribute("autocomplete", "off");
  textarea.setAttribute("autocorrect", "off");
  textarea.setAttribute("autocapitalize", "none");
  textarea.setAttribute("spellcheck", "false");
  textarea.setAttribute("enterkeyhint", "enter");
  // Best effort: the browser owns the native accessory row, so classify
  // xterm's hidden input as non-credential search-like terminal input.
  textarea.setAttribute("inputmode", "search");
  textarea.setAttribute("aria-autocomplete", "none");
  textarea.setAttribute("name", "omw-terminal-input");
  textarea.setAttribute("data-terminal-input", "true");
  textarea.setAttribute("autofill", "off");
  textarea.setAttribute("autofill-prediction", "off");
  textarea.setAttribute("data-form-type", "other");
  textarea.setAttribute("data-lpignore", "true");
  textarea.setAttribute("data-1p-ignore", "true");
  textarea.setAttribute("data-bwignore", "true");
  textarea.spellcheck = false;
}
