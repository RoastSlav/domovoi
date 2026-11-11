import "@testing-library/jest-dom/vitest";
import { configure } from "@testing-library/react";

// Markdown is fetched on first use; under a full run that first fetch can outlast the
// default second, and an answer that is merely late must not read as missing.
configure({ asyncUtilTimeout: 4000 });

// The selected host is remembered in localStorage; no test may see another's choice.
afterEach(() => {
  localStorage.clear();
});

// jsdom has no modal dialogs; opening one only needs to make its content reachable.
if (!HTMLDialogElement.prototype.showModal) {
  HTMLDialogElement.prototype.showModal = function showModal(this: HTMLDialogElement) {
    this.setAttribute("open", "");
  };
  HTMLDialogElement.prototype.close = function close(this: HTMLDialogElement) {
    this.removeAttribute("open");
  };
}
