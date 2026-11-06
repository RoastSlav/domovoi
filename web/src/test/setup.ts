import "@testing-library/jest-dom/vitest";

// The selected host is remembered in localStorage; no test may see another's choice.
afterEach(() => {
  localStorage.clear();
});
