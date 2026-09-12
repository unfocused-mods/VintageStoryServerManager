// @vitest-environment jsdom
import { afterEach, expect, it, vi } from "vitest";
import {
  cleanup,
  fireEvent,
  render,
  screen,
  within,
} from "@testing-library/react";
import "@testing-library/jest-dom/vitest";
import Advisor, {
  suitableOffers,
  sizeShortlist,
  memoryEstimate,
  type HostingOffer,
  type ExchangeRates,
  monthlyAmount,
  convertedEstimate,
} from "./Advisor";
import { Api } from "./api";
import { randomServerName } from "./serverName";
const offer: HostingOffer = {
  id: "vultr:syd:four",
  provider: "vultr",
  region: "Sydney",
  area: "oceania",
  size: "four",
  vcpus: 4,
  memory_gib: 8,
  compute: 48,
  total: 48,
  currency: "USD",
  source: "https://api.vultr.com/v2/plans",
  checked_at: "2026-09-06",
  live: true,
  shared_cpu: true,
  details: "Includes storage",
};
afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
});
it("loads options without querying cloud accounts or requesting expert fields", async () => {
  const api = new Api("http://localhost", "");
  const request = vi
    .spyOn(api, "request")
    .mockResolvedValue({ offers: [offer], notices: [] });
  render(<Advisor api={api} />);
  expect(await screen.findByText("Sydney")).toBeInTheDocument();
  expect(request).toHaveBeenCalledWith("/hosting/catalog");
  expect(screen.queryByLabelText("Size ID")).not.toBeInTheDocument();
  fireEvent.change(screen.getByLabelText("Where are your players?"), {
    target: { value: "europe" },
  });
  expect(screen.getByText(/No suitable options/)).toBeInTheDocument();
});
it("selects sufficient memory and region without treating partial costs as totals", () => {
  const large = { ...offer, id: "large", memory_gib: 16, total: null };
  expect(suitableOffers([offer, large], "oceania", 20, "heavy")).toEqual([
    large,
  ]);
  expect(suitableOffers([offer], "europe", 6, "none")).toEqual([]);
});
it("generates cloud-safe readable names", () => {
  for (let i = 0; i < 100; i++)
    expect(randomServerName()).toMatch(/^[a-z]{3,12}-[a-z]{3,12}$/);
});
it("keeps provider comparison compact and reveals alternative regions on request", async () => {
  const api = new Api("http://localhost", "");
  vi.spyOn(api, "request").mockResolvedValue({
    offers: [
      offer,
      {
        ...offer,
        id: "vultr:mel:four",
        region: "Melbourne",
        compute: 50,
        total: 50,
      },
    ],
    notices: [],
  });
  render(<Advisor api={api} />);
  expect(await screen.findByText("Sydney")).toBeInTheDocument();
  expect(
    screen.queryByLabelText("Vultr sizes & regions"),
  ).not.toBeInTheDocument();
  fireEvent.click(screen.getByRole("button", { name: "Sizes & regions" }));
  fireEvent.change(screen.getByLabelText("Vultr sizes & regions"), {
    target: { value: "vultr:mel:four" },
  });
  expect(
    screen.getByRole("cell", { name: "Melbourne 4 vCPU · 8 GiB RAM" }),
  ).toBeInTheDocument();
  fireEvent.click(screen.getByRole("button", { name: "Sizes & regions" }));
  expect(
    screen.queryByLabelText("Vultr sizes & regions"),
  ).not.toBeInTheDocument();
});

it("fits six lightly modded players in 4 GiB without rounding them up to 8 GiB", () => {
  const small = { ...offer, memory_gib: 4 };
  expect(memoryEstimate(6, "light")).toBeCloseTo(3.8);
  expect(suitableOffers([small], "", 6, "light")).toEqual([small]);
  expect(memoryEstimate(1, "none")).toBeCloseTo(1.8);
  expect(memoryEstimate(6, "heavy")).toBeCloseTo(5.3);
});
it("requires recommended CPU by default and makes economy an explicit choice", async () => {
  const economy = {
    ...offer,
    id: "small",
    vcpus: 2,
    memory_gib: 4,
    compute: 24,
    total: 24,
  };
  expect(suitableOffers([economy, offer], "", 6, "light")).toEqual([offer]);
  expect(suitableOffers([economy, offer], "", 6, "light", true)).toEqual([
    economy,
    offer,
  ]);
  const api = new Api("http://localhost", "");
  vi.spyOn(api, "request").mockResolvedValue({
    offers: [economy, offer],
    notices: [],
  });
  render(<Advisor api={api} />);
  expect(await screen.findByText("4 vCPU · 8 GiB RAM")).toBeInTheDocument();
  fireEvent.change(screen.getByLabelText("Server sizing"), {
    target: { value: "economy" },
  });
  expect(screen.getByText("2 vCPU · 4 GiB RAM")).toBeInTheDocument();
  expect(
    screen.getByText(/below Vintage Story’s recommendation of 4 threads/),
  ).toBeVisible();
});

const fx: ExchangeRates = {
  base: "EUR",
  rates: { EUR: 1, USD: 1.25, NZD: 2.5, AUD: 2 },
  as_of: "2026-09-04",
  fetched_at: "2026-09-06T00:00:00Z",
  source: "https://www.ecb.europa.eu/stats/eurofxref/eurofxref-daily.xml",
  status: "current",
};
it("converts currencies using both rates and never substitutes missing or stale rates", () => {
  expect(convertedEstimate(offer, 24, "NZD", fx)).toBe(96);
  expect(convertedEstimate({ ...offer, currency: "EUR" }, 24, "USD", fx)).toBe(
    60,
  );
  expect(convertedEstimate(offer, 24, "AUD", fx)).toBeCloseTo(76.8);
  expect(convertedEstimate(offer, 24, "USD")).toBe(48);
  expect(convertedEstimate(offer, 24, "NZD")).toBeNull();
  expect(
    convertedEstimate(offer, 24, "NZD", { ...fx, status: "stale" }),
  ).toBeNull();
  expect(
    convertedEstimate(offer, 24, "NZD", { ...fx, rates: { USD: 1.25 } }),
  ).toBeNull();
  expect(
    convertedEstimate(offer, 24, "NZD", {
      ...fx,
      rates: { ...fx.rates, USD: 0 },
    }),
  ).toBeNull();
});
it("scales only Azure compute, preserving fixed charges and other providers' monthly bills", () => {
  const azure = {
    ...offer,
    provider: "azure" as const,
    compute: 96,
    total: 116,
  };
  expect(monthlyAmount(azure, 24)).toBe(116);
  expect(monthlyAmount(azure, 6)).toBe(44);
  expect(monthlyAmount(azure, 0)).toBe(20);
  expect(monthlyAmount({ ...azure, total: null }, 6)).toBe(24);
  for (const provider of ["hetzner", "digital_ocean", "vultr"] as const) {
    expect(monthlyAmount({ ...offer, provider }, 0)).toBe(48);
    expect(monthlyAmount({ ...offer, provider }, 6)).toBe(48);
  }
});
it("uses the selected currency for every provider, regional choice and budget comparison", async () => {
  const api = new Api("http://localhost", "");
  vi.spyOn(api, "request").mockResolvedValue({
    offers: [
      { ...offer, provider: "azure", id: "azure", compute: 96, total: null },
      {
        ...offer,
        provider: "hetzner",
        id: "hetzner",
        currency: "EUR",
        compute: 24,
        total: 24,
      },
      { ...offer, id: "vultr-a" },
      { ...offer, id: "vultr-b", region: "Melbourne", compute: 50, total: 50 },
    ],
    notices: [],
    fx,
  });
  render(<Advisor api={api} />);
  expect(
    await screen.findByRole("columnheader", {
      name: "Monthly estimate - USD",
    }),
  ).toBeVisible();
  expect(screen.getByLabelText("Hours on per day")).toHaveValue(24);
  expect(
    within(screen.getByLabelText("Currency"))
      .getAllByRole("option")
      .map((o) => o.textContent),
  ).toEqual(["USD", "EUR", "NZD", "AUD"]);
  fireEvent.change(screen.getByLabelText("Currency"), {
    target: { value: "NZD" },
  });
  fireEvent.change(screen.getByLabelText("Hours on per day"), {
    target: { value: "8" },
  });
  fireEvent.change(screen.getByLabelText("Monthly budget"), {
    target: { value: "70" },
  });
  expect(
    screen.getByRole("columnheader", { name: "Monthly estimate - NZD" }),
  ).toBeVisible();
  expect(screen.getByText(/NZD\s+64\.00/)).toBeVisible();
  expect(screen.getByText(/NZD\s+60\.00/)).toBeVisible();
  expect(screen.getByText(/NZD\s+96\.00/)).toBeVisible();
  expect(screen.getAllByText("Above budget")).toHaveLength(1);
  expect(screen.getByText("8 h/day; deallocate when off")).toBeVisible();
  expect(screen.getAllByText("Still billed while off")).toHaveLength(2);
  const row = screen.getByRole("rowheader", { name: /Vultr/ }).closest("tr")!;
  fireEvent.click(
    within(row).getByRole("button", { name: "Sizes & regions" }),
  );
  expect(
    within(screen.getByLabelText("Vultr sizes & regions")).getByRole("option", {
      name: /Melbourne.*NZD\s+100\.00/,
    }),
  ).toBeVisible();
});
it("shows original prices and unranked budgets when conversion is unavailable", async () => {
  const api = new Api("http://localhost", "");
  vi.spyOn(api, "request").mockResolvedValue({
    offers: [offer],
    notices: [],
    fx: { ...fx, status: "stale" },
  });
  render(<Advisor api={api} />);
  await screen.findByText("Sydney");
  fireEvent.change(screen.getByLabelText("Currency"), {
    target: { value: "NZD" },
  });
  fireEvent.change(screen.getByLabelText("Monthly budget"), {
    target: { value: "1" },
  });
  expect(screen.getByText("Unavailable in NZD")).toBeVisible();
  expect(screen.getByText(/Original: USD\s+48\.00/)).toBeVisible();
  expect(screen.getByText("Budget comparison unavailable")).toBeVisible();
  expect(screen.queryByText("Above budget")).not.toBeInTheDocument();
  expect(screen.getByText(/Exchange rates are out of date/)).toBeVisible();
});

it("offers a size spread and reveals equivalent catalog sizes on demand", async () => {
  const api = new Api("http://localhost", "");
  vi.spyOn(api, "request").mockResolvedValue({ offers: [
    offer,
    { ...offer, id: "same-capacity", size: "alternative-family", total: 49 },
    { ...offer, id: "medium", size: "medium", vcpus: 8, memory_gib: 16, total: 80 },
    { ...offer, id: "large", size: "large", vcpus: 16, memory_gib: 32, total: 160 },
    { ...offer, id: "unsuitable", size: "too-small", vcpus: 1, memory_gib: 1, total: 5 },
  ], notices: [] });
  render(<Advisor api={api} />);
  fireEvent.click(await screen.findByRole("button", { name: "Sizes & regions" }));
  const select = screen.getByLabelText("Vultr sizes & regions");
  expect(within(select).getAllByRole("option")).toHaveLength(2);
  expect(within(select).getByRole("option", { name: /8 vCPU.*16 GiB/ })).toBeInTheDocument();
  expect(within(select).queryByRole("option", { name: /alternative-family/ })).not.toBeInTheDocument();
  fireEvent.click(screen.getByLabelText("Show all 4 suitable Vultr options"));
  expect(within(select).getAllByRole("option")).toHaveLength(4);
  fireEvent.change(select, { target: { value: "same-capacity" } });
  expect(screen.getByText(/alternative-family · Shared CPU/)).toBeInTheDocument();
  expect(within(select).queryByRole("option", { name: /too-small/ })).not.toBeInTheDocument();
});

it("bounds headroom to the workload while retaining the cheapest suitable offer", () => {
  const rows = [8, 12, 16, 24, 32, 1024].map((memory, i) => ({ ...offer, id: `size-${i}`, memory_gib: memory }));
  expect(sizeShortlist(rows, 6, "light").map((row) => row.memory_gib)).toEqual([8, 12, 16]);
  const largerWorkload = suitableOffers(rows, "", 40, "heavy");
  expect(sizeShortlist(largerWorkload, 40, "heavy").map((row) => row.memory_gib)).toEqual([16, 24, 32]);
  const cheapUpgrade = { ...rows[2], id: "discounted", total: 1 };
  expect(sizeShortlist([cheapUpgrade, ...rows], 6, "light")[0]).toBe(cheapUpgrade);
  expect(sizeShortlist(rows, 40, "heavy").some((row) => row.memory_gib === 1024)).toBe(false);
  const elsewhere = { ...rows[0], id: "elsewhere", region: "Melbourne" };
  expect(sizeShortlist([...rows, elsewhere])).toContain(elsewhere);
});
