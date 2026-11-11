import "@testing-library/jest-dom/vitest";

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
