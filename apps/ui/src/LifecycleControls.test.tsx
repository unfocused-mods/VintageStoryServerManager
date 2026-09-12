// @vitest-environment jsdom
import { afterEach, expect, it, vi } from "vitest";
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import "@testing-library/jest-dom/vitest";
import LifecycleControls from "./LifecycleControls";
afterEach(cleanup);
it.each([
  ["running", ["Stop", "Restart"]],
  ["stopped", ["Start server"]],
  ["sleeping", ["Start server"]],
  ["deallocated", ["Start VM"]],
  ["awaiting_bootstrap", ["Finish server setup"]],
  ["attention", ["Check server setup"]],
])("shows only relevant actions for %s", (state, names) => {
  render(
    <LifecycleControls
      state={state}
      busy={false}
      act={vi.fn()}
      setup={vi.fn()}
    />,
  );
  expect(screen.getAllByRole("button").map((b) => b.textContent)).toEqual(
    names,
  );
});
it("starts the VM instead of sending a game command to a deallocated host", () => {
  const act = vi.fn();
  render(
    <LifecycleControls
      state="deallocated"
      busy={false}
      act={act}
      setup={vi.fn()}
    />,
  );
  fireEvent.click(screen.getByRole("button", { name: "Start VM" }));
  expect(act).toHaveBeenCalledWith("cloud-start");
});
it("shows progress without conflicting controls during startup", () => {
  render(
    <LifecycleControls
      state="starting"
      busy={false}
      act={vi.fn()}
      setup={vi.fn()}
    />,
  );
  expect(
    screen.getByRole("button", { name: "Starting server…" }),
  ).toBeDisabled();
  expect(screen.getAllByRole("button")).toHaveLength(1);
});
