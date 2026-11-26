//! The built-in styles every screen starts from. A game's own CSS always
//! wins over these, whatever its specificity.

/// The user-agent sheet. Tags that are inline text (`span`, `label`, `br`)
/// get that from the cascade, not from here, and the `hidden` attribute is
/// handled in code, because neither can be written in the CSS subset.
pub const UA_CSS: &str = "\
head, title, style, link, option { display: none; }
body { margin: 0; }
h1 { font-size: 2em; font-weight: bold; margin: 0.67em 0; }
h2 { font-size: 1.5em; font-weight: bold; margin: 0.83em 0; }
h3 { font-size: 1.17em; font-weight: bold; margin: 1em 0; }
p { margin: 1em 0; }
button, input, select {
  padding: 8px 16px;
  border: 2px solid rgba(255, 255, 255, 0.55);
  border-radius: 6px;
  background-color: rgba(255, 255, 255, 0.12);
  color: #ffffff;
}
button { text-align: center; cursor: pointer; }
select { cursor: pointer; }
input { cursor: text; }
button:hover, select:hover { background-color: rgba(255, 255, 255, 0.22); }
button:active { background-color: rgba(255, 255, 255, 0.32); }
progress {
  height: 12px;
  border-radius: 6px;
  background-color: rgba(255, 255, 255, 0.18);
}
:focus { outline: 3px solid #ffd75e; outline-offset: 2px; }
:disabled { opacity: 0.5; cursor: not-allowed; }
";
