export function configureTerminalInputTraits(root: HTMLElement): void {
  const textarea = root.querySelector("textarea");
  if (!(textarea instanceof HTMLTextAreaElement)) return;

  textarea.setAttribute("autocomplete", "off");
  textarea.setAttribute("autocorrect", "off");
  textarea.setAttribute("autocapitalize", "none");
  textarea.setAttribute("spellcheck", "false");
  textarea.setAttribute("enterkeyhint", "enter");
  textarea.spellcheck = false;
}
