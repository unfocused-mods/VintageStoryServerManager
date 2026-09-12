// Apply the saved/system theme before first paint. The interactive selector owns later updates.
(() => {
  let preference;
  try {
    preference = localStorage.getItem("vsm.theme");
  } catch {
    /* Storage is optional. */
  }
  const theme =
    preference === "light" || preference === "dark"
      ? preference
      : matchMedia("(prefers-color-scheme: dark)").matches
        ? "dark"
        : "light";
  document.documentElement.dataset.theme = theme;
  document
    .querySelector('meta[name="theme-color"]')
    ?.setAttribute("content", theme === "dark" ? "#101010" : "#f7f7f7");
})();
