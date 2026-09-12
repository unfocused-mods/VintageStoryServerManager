// @vitest-environment jsdom
import { afterEach, describe, expect, it, vi } from "vitest";
import {
  act,
  cleanup,
  fireEvent,
  render,
  screen,
} from "@testing-library/react";
import "@testing-library/jest-dom/vitest";
import ThemeSelect from "./ThemeSelect";
function systemTheme(dark: boolean) {
  let listener: (() => void) | undefined;
  const media = {
    matches: dark,
    addEventListener: vi.fn((_name: string, fn: () => void) => {
      listener = fn;
    }),
    removeEventListener: vi.fn(),
  };
  vi.stubGlobal(
    "matchMedia",
    vi.fn(() => media),
  );
  return {
    change(value: boolean) {
      act(() => {
        media.matches = value;
        listener?.();
      });
    },
    media,
  };
}
afterEach(() => {
  cleanup();
  localStorage.clear();
  vi.unstubAllGlobals();
});
describe("color theme", () => {
  it("follows the system by default and reacts to system changes", () => {
    const system = systemTheme(true);
    render(<ThemeSelect />);
    expect(document.documentElement.dataset.theme).toBe("dark");
    expect(screen.getByLabelText("Color theme")).toHaveValue("system");
    system.change(false);
    expect(document.documentElement.dataset.theme).toBe("light");
    expect(localStorage.getItem("vsm.theme")).toBeNull();
  });
  it("keeps a stored explicit preference when the system changes", () => {
    localStorage.setItem("vsm.theme", "light");
    const system = systemTheme(true);
    render(<ThemeSelect />);
    expect(document.documentElement.dataset.theme).toBe("light");
    system.change(false);
    system.change(true);
    expect(document.documentElement.dataset.theme).toBe("light");
    expect(screen.getByLabelText("Color theme")).toHaveValue("light");
  });
  it("persists an override and can return to following the system", () => {
    const system = systemTheme(false);
    render(<ThemeSelect />);
    fireEvent.change(screen.getByLabelText("Color theme"), {
      target: { value: "dark" },
    });
    expect(localStorage.getItem("vsm.theme")).toBe("dark");
    expect(document.documentElement.dataset.theme).toBe("dark");
    fireEvent.change(screen.getByLabelText("Color theme"), {
      target: { value: "system" },
    });
    expect(localStorage.getItem("vsm.theme")).toBeNull();
    expect(document.documentElement.dataset.theme).toBe("light");
    cleanup();
    expect(system.media.removeEventListener).toHaveBeenCalled();
  });
});
