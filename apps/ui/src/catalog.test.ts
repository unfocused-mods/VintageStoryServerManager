import { describe, expect, it } from "vitest";
import { catalogOptions, instanceChoices } from "./catalog";

const azureSize = (name = "Standard_D4s_v5", overrides = {}) => ({
  name,
  resourceType: "virtualMachines",
  locations: ["australiaeast"],
  restrictions: [],
  capabilities: [
    { name: "CpuArchitectureType", value: "x64" },
    { name: "vCPUs", value: "4" },
    { name: "MemoryGB", value: "16" },
  ],
  ...overrides,
});

describe("catalogOptions", () => {
  it("matches mixed-case New Zealand North SKU locations without relaxing architecture or restrictions", () => {
    const result = catalogOptions(
      "azure",
      {
        regions: [
          { name: "newzealandnorth", displayName: "New Zealand North" },
        ],
        sizes: [
          azureSize("Standard_D2s_v5", {
            locations: ["NewZealandNorth"],
            capabilities: [
              { name: "CpuArchitectureType", value: "x64" },
              { name: "vCPUs", value: "2" },
              { name: "MemoryGB", value: "8" },
            ],
          }),
          azureSize("Standard_F4s_v2", {
            locations: ["NewZealandNorth"],
            restrictions: [{ type: "Location", values: ["NEWZEALANDNORTH"] }],
          }),
          azureSize("Standard_D4s_v5", {
            locations: ["NewZealandNorth"],
            capabilities: [],
          }),
        ],
      },
      "newzealandnorth",
    );
    expect(result.allSizes).toHaveLength(1);
    expect(result.allSizes[0]).toMatchObject({
      value: "Standard_D2s_v5",
      vcpus: 2,
      memoryGiB: 8,
    });
    expect(result.regions[0]).toEqual({
      value: "newzealandnorth",
      label: "New Zealand North",
    });
  });
  it("uses Azure display names and only regional x64 D/F VM sizes", () => {
    const result = catalogOptions(
      "azure",
      {
        regions: [
          { name: "australiaeast", displayName: "Australia East" },
          { name: "global", metadata: { regionType: "Logical" } },
        ],
        sizes: [
          azureSize(),
          azureSize("Standard_F4s_v2"),
          azureSize("Standard_B4ms"),
          azureSize("Standard_D4ps_v5", {
            capabilities: [{ name: "CpuArchitectureType", value: "Arm64" }],
          }),
          azureSize("Standard_D8s_v5", { locations: ["westus"] }),
          azureSize("Standard_D2s_v5", { resourceType: "disks" }),
          azureSize("Standard_D16s_v5", { capabilities: [] }),
        ],
      },
      "australiaeast",
    );
    expect(result.regions).toEqual([
      { value: "australiaeast", label: "Australia East" },
    ]);
    expect(result.allSizes.map((s) => s.value)).toEqual([
      "Standard_D4s_v5",
      "Standard_F4s_v2",
    ]);
    expect(result.sizes[0].label).toBe("Standard_D4s_v5 · 4 vCPU · 16 GiB RAM");
  });

  it("excludes Azure location restrictions from either response field but allows zone-only restrictions", () => {
    const result = catalogOptions(
      "azure",
      {
        sizes: [
          azureSize("Standard_D2s_v5", {
            restrictions: [{ type: "Location", values: ["australiaeast"] }],
          }),
          azureSize("Standard_D4s_v5", {
            restrictions: [
              {
                type: "Location",
                restrictionInfo: { locations: ["australiaeast"] },
              },
            ],
          }),
          azureSize("Standard_D8s_v5", {
            restrictions: [{ type: "Location" }],
          }),
          azureSize("Standard_D16s_v5", {
            restrictions: [
              {
                type: "Zone",
                values: ["australiaeast"],
                restrictionInfo: { zones: ["1"] },
              },
            ],
          }),
          azureSize("Standard_D32s_v5", {
            restrictions: [{ type: "Location", values: ["westus"] }],
          }),
        ],
      },
      "AustraliaEast",
    );
    expect(result.allSizes.map((s) => s.value)).toEqual([
      "Standard_D16s_v5",
      "Standard_D32s_v5",
    ]);
  });

  it("uses DigitalOcean availability and regional membership, converting memory units", () => {
    const valid = {
      slug: "s-2vcpu-4gb",
      available: true,
      regions: ["syd1"],
      vcpus: 2,
      memory: 4096,
    };
    const result = catalogOptions(
      "digital_ocean",
      {
        regions: [
          { slug: "syd1", name: "Sydney 1", available: true },
          { slug: "old", available: false },
        ],
        sizes: [
          valid,
          { ...valid, slug: "unavailable", available: false },
          { ...valid, slug: "elsewhere", regions: ["nyc1"] },
          { ...valid, slug: "unknown", available: undefined },
        ],
      },
      "syd1",
    );
    expect(result.regions).toEqual([{ value: "syd1", label: "Sydney 1" }]);
    expect(result.sizes).toEqual([
      {
        value: valid.slug,
        label: "s-2vcpu-4gb · 2 vCPU · 4 GiB RAM",
        vcpus: 2,
        memoryGiB: 4,
        recommendation: "economy",
      },
    ]);
  });

  it("uses Hetzner regional price listings without treating them as a capacity guarantee", () => {
    const valid = {
      name: "cx23",
      architecture: "x86",
      cores: 2,
      memory: 4,
      prices: [{ location: "fsn1" }],
    };
    const result = catalogOptions(
      "hetzner",
      {
        regions: [{ name: "fsn1", description: "Falkenstein" }],
        sizes: [
          valid,
          { ...valid, name: "cax11", architecture: "arm" },
          { ...valid, name: "unknown", prices: [] },
          { ...valid, name: "old", deprecated: true },
        ],
      },
      "fsn1",
    );
    expect(result.allSizes.map((s) => s.value)).toEqual(["cx23"]);
    expect(result.sizes[0].label).not.toMatch(/available/i);
  });

  it("honors modern Hetzner per-location availability and deprecation", () => {
    const base = { architecture: "x86", prices: [{ location: "fsn1" }] };
    const result = catalogOptions(
      "hetzner",
      {
        sizes: [
          {
            ...base,
            name: "active",
            locations: [{ name: "fsn1", available: true }],
          },
          {
            ...base,
            name: "full",
            locations: [{ name: "fsn1", available: false }],
          },
          {
            ...base,
            name: "retiring",
            locations: [
              {
                name: "fsn1",
                available: true,
                deprecation: { unavailable_after: "2027-01-01" },
              },
            ],
          },
          {
            ...base,
            name: "elsewhere",
            locations: [{ name: "hel1", available: true }],
          },
          { ...base, name: "unknown", locations: [{ name: "fsn1" }] },
        ],
      },
      "fsn1",
    );
    expect(result.allSizes.map((s) => s.value)).toEqual(["active"]);
  });

  it("requires Vultr location membership and omits unsupported plan categories", () => {
    const valid = {
      id: "vc2-2c-4gb",
      type: "vc2",
      locations: ["syd"],
      vcpu_count: 2,
      ram: 4096,
    };
    const result = catalogOptions(
      "vultr",
      {
        regions: [{ id: "syd", city: "Sydney", country: "AU" }],
        sizes: [
          valid,
          { ...valid, id: "gpu", type: "vcg" },
          { ...valid, id: "unknown", locations: undefined },
        ],
      },
      "syd",
    );
    expect(result.regions).toEqual([{ value: "syd", label: "Sydney, AU" }]);
    expect(result.allSizes.map((s) => s.value)).toEqual([valid.id]);
  });

  it("does not invent defaults for missing catalogs or unselected regions", () => {
    expect(catalogOptions("azure", null, "")).toEqual({
      regions: [],
      sizes: [],
      allSizes: [],
    });
    expect(catalogOptions("azure", { sizes: [azureSize()] }, "").sizes).toEqual(
      [],
    );
    expect(catalogOptions("ssh", { regions: ["anything"] }, "any")).toEqual({
      regions: [],
      sizes: [],
      allSizes: [],
    });
  });
});

function doSize(slug: string, cpu: number, memory: number, price: number) {
  return {
    slug,
    vcpus: cpu,
    memory: memory * 1024,
    price_monthly: price,
    available: true,
    regions: ["syd1"],
  };
}
it("shortlists distinct economical, recommended and headroom capacity instead of eight equal-size SKUs", () => {
  const catalog = {
    sizes: [
      doSize("small", 2, 4, 24),
      ...Array.from({ length: 8 }, (_, i) =>
        doSize(`medium-${i}`, 4, 8, 48 + i),
      ),
      doSize("large", 4, 16, 80),
      doSize("huge", 16, 64, 300),
    ],
  };
  const result = catalogOptions("digital_ocean", catalog, "syd1");
  expect(result.sizes.map((s) => [s.value, s.recommendation])).toEqual([
    ["small", "economy"],
    ["medium-0", "recommended"],
    ["large", "headroom"],
  ]);
  expect(result.allSizes).toHaveLength(11);
  expect(
    instanceChoices(result, false, "medium-7").map((s) => s.value),
  ).toContain("medium-7");
  expect(instanceChoices(result, true, "medium-7")).toHaveLength(11);
});
it("changes suitability with players and mods and keeps unsuitable options only in the full catalog", () => {
  const raw = {
    sizes: [
      doSize("small", 2, 4, 24),
      doSize("medium", 4, 8, 48),
      doSize("large", 4, 16, 80),
    ],
  };
  expect(
    catalogOptions("digital_ocean", raw, "syd1", {
      players: 6,
      mods: "light",
    }).sizes.map((s) => s.value),
  ).toContain("small");
  expect(
    catalogOptions("digital_ocean", raw, "syd1", {
      players: 20,
      mods: "heavy",
    }).sizes.map((s) => s.value),
  ).toEqual(["large"]);
  const insufficient = catalogOptions("digital_ocean", raw, "syd1", {
    players: 100,
    mods: "heavy",
  });
  expect(insufficient.sizes).toEqual([]);
  expect(insufficient.allSizes).toHaveLength(3);
  expect(catalogOptions("digital_ocean", raw, "elsewhere").allSizes).toEqual(
    [],
  );
});
