// @vitest-environment jsdom
import { afterEach, expect, it, vi } from "vitest";
import {
  act,
  cleanup,
  fireEvent,
  render,
  screen,
} from "@testing-library/react";
import "@testing-library/jest-dom/vitest";
import { Api, type Server } from "./api";
import WorldCreator, { worldHelpText } from "./WorldCreator";
afterEach(cleanup);
const server = {
  id: "one",
  state: "stopped",
  game_version: "1.22.7",
} as Server;
const schema = {
  version: "1.22.7",
  source: "Installed game metadata",
  presets: [
    {
      code: "surviveandbuild",
      label: "Standard",
      worldType: "standard",
      worldConfig: { hunger: "1", map: true },
    },
    {
      code: "exploration",
      label: "Exploration",
      worldType: "standard",
      worldConfig: { hunger: "0.5", map: true },
    },
  ],
  fields: [
    {
      code: "hunger",
      label: "Hunger rate",
      category: "survival",
      categoryLabel: "Survival",
      dataType: "dropdown",
      values: ["0.5", "1"],
      names: ["Slower", "Normal"],
      default: "1",
    },
    {
      code: "map",
      label: "Allow map",
      category: "survival",
      categoryLabel: "Survival",
      dataType: "bool",
      default: true,
    },
  ],
};
it("uses installed options, preset defaults and typed overrides without exposing raw JSON", async () => {
  const api = new Api("http://localhost", "");
  vi.spyOn(api, "request").mockResolvedValue(schema);
  const act = vi.fn();
  render(<WorldCreator api={api} server={server} act={act} />);
  await screen.findByLabelText("World name");
  fireEvent.change(screen.getByLabelText("World name"), {
    target: { value: "My world" },
  });
  fireEvent.change(screen.getByLabelText("Play style"), {
    target: { value: "exploration" },
  });
  expect(screen.getByLabelText("Hunger rate")).toHaveValue("0.5");
  fireEvent.click(screen.getByLabelText("Allow map"));
  fireEvent.click(
    screen.getByRole("button", { name: "Create and select world" }),
  );
  expect(act).toHaveBeenCalledWith("world-create", {
    name: "My world",
    seed: null,
    play_style: "exploration",
    height: 256,
    world_configuration: { map: false },
  });
});
it("does not fabricate controls when installed metadata cannot be loaded", async () => {
  const api = new Api("http://localhost", "");
  vi.spyOn(api, "request").mockRejectedValue(
    new Error("Game metadata unavailable"),
  );
  render(<WorldCreator api={api} server={server} act={vi.fn()} />);
  expect(await screen.findByRole("alert")).toHaveTextContent(
    "Game metadata unavailable",
  );
  expect(screen.queryByLabelText("Play style")).not.toBeInTheDocument();
});

it("ignores a stale response after switching servers", async () => {
  const api = new Api("http://localhost", "");
  let resolveOld!: (v: unknown) => void;
  vi.spyOn(api, "request")
    .mockImplementationOnce(
      () =>
        new Promise((resolve) => {
          resolveOld = resolve;
        }),
    )
    .mockResolvedValueOnce({
      ...schema,
      presets: [{ ...schema.presets[1], label: "New server preset" }],
    });
  const view = render(<WorldCreator api={api} server={server} act={vi.fn()} />);
  view.rerender(
    <WorldCreator api={api} server={{ ...server, id: "two" }} act={vi.fn()} />,
  );
  await screen.findByRole("option", { name: "New server preset" });
  await act(async () => resolveOld(schema));
  expect(
    screen.getByRole("option", { name: "New server preset" }),
  ).toBeInTheDocument();
  expect(
    screen.queryByRole("option", { name: "Standard" }),
  ).not.toBeInTheDocument();
});

it("shows game help as plain text without rendering its VTML formatting", () => {
  expect(
    worldHelpText(
      'Climate <font color="orange">Only applies here</font><br>Next line',
    ),
  ).toBe("Climate Only applies here Next line");
});
