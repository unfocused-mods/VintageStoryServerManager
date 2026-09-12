import { useEffect, useState } from "react";
export type ThemePreference = "system" | "light" | "dark";
const storageKey = "vsm.theme";
function savedPreference(): ThemePreference {
  try {
    const value = localStorage.getItem(storageKey);
    return value === "light" || value === "dark" ? value : "system";
  } catch {
    return "system";
  }
}
export function useTheme() {
  const [preference, setPreference] =
    useState<ThemePreference>(savedPreference);
  const [systemDark, setSystemDark] = useState(
    () => window.matchMedia?.("(prefers-color-scheme: dark)").matches ?? false,
  );
  const theme =
    preference === "system" ? (systemDark ? "dark" : "light") : preference;
  useEffect(() => {
    const media = window.matchMedia?.("(prefers-color-scheme: dark)");
    if (!media) return;
    const update = () => setSystemDark(media.matches);
    update();
    media.addEventListener("change", update);
    return () => media.removeEventListener("change", update);
  }, []);
  useEffect(() => {
    document.documentElement.dataset.theme = theme;
    document
      .querySelector('meta[name="theme-color"]')
      ?.setAttribute("content", theme === "dark" ? "#101010" : "#f7f7f7");
  }, [theme]);
  function chooseTheme(value: ThemePreference) {
    setPreference(value);
    try {
      if (value === "system") localStorage.removeItem(storageKey);
      else localStorage.setItem(storageKey, value);
    } catch {
      /* Storage may be disabled; keep the session preference. */
    }
  }
  return { preference, theme, chooseTheme };
}
export default function ThemeSelect() {
  const { preference, chooseTheme } = useTheme();
  return (
    <label className="theme-select">
      <span className="sr-only">Color theme</span>
      <select
        aria-label="Color theme"
        value={preference}
        onChange={(e) => chooseTheme(e.target.value as ThemePreference)}
      >
        <option value="system">System</option>
        <option value="light">Light</option>
        <option value="dark">Dark</option>
      </select>
    </label>
  );
}
