import "@testing-library/jest-dom/vitest";
import { configure } from "@testing-library/react";
import { expectNoA11yViolations, takeSkip } from "./axe";

// Markdown is fetched on first use; under a full run that first fetch can outlast the
// default second, and an answer that is merely late must not read as missing.
configure({ asyncUtilTimeout: 4000 });

// The selected host is remembered in localStorage; no test may see another's choice.
afterEach(() => {
  if (typeof localStorage !== "undefined") {
    localStorage.clear();
  }
});

// jsdom has no modal dialogs; opening one only needs to make its content reachable.
if (typeof HTMLDialogElement !== "undefined" && !HTMLDialogElement.prototype.showModal) {
  HTMLDialogElement.prototype.showModal = function showModal(this: HTMLDialogElement) {
    this.setAttribute("open", "");
  };
  HTMLDialogElement.prototype.close = function close(this: HTMLDialogElement) {
    this.removeAttribute("open");
  };
}

// Every page test doubles as an accessibility check: whatever the test left on screen must
// pass axe. A test that deliberately renders something broken says so with `skipAxe()`.
afterEach(async () => {
  const skipped = takeSkip();
  const path = expect.getState().testPath ?? "";
  const isUiTest = /\/src\/(features|app|dev|components)\//.test(path.replaceAll("\\", "/"));
  if (!skipped && isUiTest && typeof document !== "undefined" && document.body.firstChild) {
    await expectNoA11yViolations(document.body);
  }
});
