// @vitest-environment jsdom
import { useState } from "react";
import { afterEach, expect, it } from "vitest";
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import "@testing-library/jest-dom/vitest";
import ServerConfigForm, { validateServerConfig } from "./ServerConfigForm";
afterEach(cleanup);
const initial = {
  ServerName: "World",
  MaxClients: 16,
  WhitelistMode: 0,
  Password: null,
  ModPaths: ["/managed/Mods"],
  WorldConfig: { SaveFileLocation: "/managed/world.vcdbs", extra: true },
  unknownModSetting: { enabled: true },
};
function Form() {
  const [value, setValue] = useState(JSON.stringify(initial));
  return (
    <>
      <ServerConfigForm value={value} onChange={setValue} />
      <output data-testid="saved">{value}</output>
    </>
  );
}
it("edits player and rule settings while preserving unknown and managed data", () => {
  render(<Form />);
  fireEvent.click(screen.getByText("Players and access"));
  fireEvent.change(screen.getByLabelText("Player limit"), {
    target: { value: "24" },
  });
  fireEvent.change(screen.getByLabelText("Whitelist"), {
    target: { value: "2" },
  });
  expect(JSON.parse(screen.getByTestId("saved").textContent!)).toEqual({
    ...initial,
    MaxClients: 24,
    WhitelistMode: 2,
  });
  expect(screen.queryByLabelText("SaveFileLocation")).not.toBeInTheDocument();
  expect(screen.queryByLabelText("StartupCommands")).not.toBeInTheDocument();
});
it("preserves null passwords and unknown boolean defaults until changed", () => {
  render(<Form />);
  fireEvent.click(screen.getByText("Players and access"));
  const password = screen.getByLabelText("Server password (optional)");
  expect(password).toHaveAttribute("type", "password");
  fireEvent.change(password, { target: { value: "private" } });
  fireEvent.change(password, { target: { value: "" } });
  expect(
    JSON.parse(screen.getByTestId("saved").textContent!).Password,
  ).toBeNull();
  expect(
    JSON.parse(screen.getByTestId("saved").textContent!),
  ).not.toHaveProperty("VerifyPlayerAuth");
});
it("validates known types, limits, enum values and malformed JSON without rejecting unknown fields", () => {
  expect(validateServerConfig(JSON.stringify(initial))).toEqual([]);
  const issues = validateServerConfig(
    JSON.stringify({
      MaxClients: -1,
      MaxChunkRadius: 2.5,
      TickTime: 0,
      VerifyPlayerAuth: "yes",
      WhitelistMode: 3,
      ServerUrl: "not a URL",
    }),
  );
  expect(issues).toHaveLength(6);
  expect(validateServerConfig("[]")).toEqual([
    "Configuration must be a valid JSON object.",
  ]);
  expect(validateServerConfig("{bad")).toHaveLength(1);
});
it("keeps a cleared numeric field invalid instead of silently deleting it from a merge update", () => {
  render(<Form />);
  fireEvent.click(screen.getByText("Players and access"));
  fireEvent.change(screen.getByLabelText("Player limit"), {
    target: { value: "" },
  });
  const value = screen.getByTestId("saved").textContent!;
  expect(JSON.parse(value)).toHaveProperty("MaxClients", "");
  expect(validateServerConfig(value)[0]).toContain("Player limit");
  expect(screen.getByRole("alert")).toBeVisible();
});
