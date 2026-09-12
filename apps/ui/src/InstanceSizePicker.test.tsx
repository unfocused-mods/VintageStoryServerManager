// @vitest-environment jsdom
import { useState } from "react";
import { afterEach, expect, it } from "vitest";
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import "@testing-library/jest-dom/vitest";
import InstanceSizePicker from "./InstanceSizePicker";
import { catalogOptions } from "./catalog";
afterEach(cleanup);
const options = catalogOptions(
  "digital_ocean",
  {
    sizes: [
      { slug: "small", vcpus: 2, memory: 4096, price_monthly: 24 },
      { slug: "medium", vcpus: 4, memory: 8192, price_monthly: 48 },
      { slug: "medium-premium", vcpus: 4, memory: 8192, price_monthly: 60 },
      { slug: "large", vcpus: 4, memory: 16384, price_monthly: 80 },
    ].map((s) => ({ ...s, available: true, regions: ["syd1"] })),
  },
  "syd1",
);
function Form() {
  const [showAll, setShowAll] = useState(false),
    [value, setValue] = useState("medium");
  return (
    <InstanceSizePicker
      options={options}
      value={value}
      onChange={setValue}
      showAll={showAll}
      onShowAll={setShowAll}
    />
  );
}
it("puts Show all beside the selector and retains a chosen non-shortlist size when collapsed", () => {
  render(<Form />);
  expect(
    screen.queryByRole("option", { name: /medium-premium/ }),
  ).not.toBeInTheDocument();
  fireEvent.click(screen.getByRole("button", { name: "Show all 4 sizes" }));
  fireEvent.change(screen.getByLabelText("Instance size"), {
    target: { value: "medium-premium" },
  });
  fireEvent.click(screen.getByRole("button", { name: "Show suggested sizes" }));
  expect(screen.getByLabelText("Instance size")).toHaveValue("medium-premium");
  expect(
    screen.getByRole("option", { name: /Selected — medium-premium/ }),
  ).toBeVisible();
});
