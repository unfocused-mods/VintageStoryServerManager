import { memoryEstimate } from "./sizing";
export interface CatalogOption {
  value: string;
  label: string;
  monthlyPrice?: number;
  currency?: string;
  vcpus?: number;
  memoryGiB?: number;
  recommendation?: "economy" | "recommended" | "headroom";
}
export interface CatalogOptions {
  regions: CatalogOption[];
  sizes: CatalogOption[];
  allSizes: CatalogOption[];
}
type Row = Record<string, unknown>;
const row = (value: unknown): Row =>
  value !== null && typeof value === "object" && !Array.isArray(value)
    ? (value as Row)
    : {};
const rows = (value: unknown): Row[] =>
  Array.isArray(value) ? value.map(row) : [];
const text = (value: unknown): string =>
  typeof value === "string" ? value : "";
const strings = (value: unknown): string[] =>
  Array.isArray(value)
    ? value.filter((v): v is string => typeof v === "string")
    : [];
const has = (value: unknown, region: string) =>
  strings(value).some((v) => v.toLowerCase() === region.toLowerCase());
const number = (value: unknown) => {
  const n =
    typeof value === "number" ||
    (typeof value === "string" && value.trim() !== "")
      ? Number(value)
      : NaN;
  return Number.isFinite(n) && n > 0 ? n : undefined;
};
function option(value: unknown, label: unknown = value): CatalogOption {
  return { value: text(value), label: text(label) || text(value) };
}
function unique(options: CatalogOption[]): CatalogOption[] {
  return [
    ...new Map(
      options.filter((o) => o.value).map((o) => [o.value, o]),
    ).values(),
  ].sort((a, b) =>
    a.label.localeCompare(b.label, undefined, { numeric: true }),
  );
}
function size(
  name: unknown,
  cpu: unknown,
  memory: unknown,
  divisor = 1,
  monthlyPrice?: number,
  currency?: string,
): CatalogOption {
  const cores = number(cpu);
  const gib = number(memory);
  const details = [
    cores ? `${cores} vCPU` : "",
    gib ? `${Number((gib / divisor).toFixed(2))} GiB RAM` : "",
  ].filter(Boolean);
  const price = Number.isFinite(monthlyPrice)
    ? ` · ${currency || "USD"} ${monthlyPrice!.toFixed(2)}/mo`
    : "";
  return {
    ...option(name, [text(name), ...details].join(" · ") + price),
    ...(Number.isFinite(monthlyPrice) ? { monthlyPrice, currency } : {}),
    vcpus: cores,
    memoryGiB: gib === undefined ? undefined : gib / divisor,
  };
}
function recommended(
  sizes: CatalogOption[],
  players: number,
  mods: string,
): CatalogOption[] {
  const memory = memoryEstimate(players, mods);
  const known = sizes.filter(
    (s) =>
      s.vcpus !== undefined &&
      s.memoryGiB !== undefined &&
      s.memoryGiB >= memory,
  );
  const compare = (a: CatalogOption, b: CatalogOption) => {
    if (
      a.monthlyPrice !== undefined &&
      b.monthlyPrice !== undefined &&
      a.currency === b.currency &&
      a.monthlyPrice !== b.monthlyPrice
    )
      return a.monthlyPrice - b.monthlyPrice;
    return (
      a.memoryGiB! - b.memoryGiB! ||
      a.vcpus! - b.vcpus! ||
      a.value.localeCompare(b.value, undefined, { numeric: true })
    );
  };
  // Multiple CPU generations with identical capacity should not fill the shortlist.
  const representatives = new Map<string, CatalogOption>();
  for (const candidate of known) {
    const key = `${candidate.vcpus}:${candidate.memoryGiB}`;
    const existing = representatives.get(key);
    if (!existing || compare(candidate, existing) < 0)
      representatives.set(key, candidate);
  }
  const pool = [...representatives.values()];
  const economy = pool
    .filter((s) => s.vcpus! >= 2 && s.vcpus! < 4)
    .sort(compare)[0];
  const standard = pool.filter((s) => s.vcpus! >= 4).sort(compare)[0];
  const base = standard || economy;
  const larger = base
    ? pool.filter(
        (s) =>
          s.value !== base.value &&
          s.vcpus! >= base.vcpus! &&
          s.memoryGiB! > base.memoryGiB!,
      )
    : [];
  const headroom =
    larger.sort(compare)[0] ||
    (base
      ? pool
          .filter(
            (s) => s.vcpus! > base.vcpus! && s.memoryGiB! >= base.memoryGiB!,
          )
          .sort(compare)[0]
      : undefined);
  return [
    economy && { ...economy, recommendation: "economy" as const },
    standard && { ...standard, recommendation: "recommended" as const },
    headroom && { ...headroom, recommendation: "headroom" as const },
  ].filter(
    (
      s,
    ): s is CatalogOption & {
      recommendation: "economy" | "recommended" | "headroom";
    } => !!s,
  );
}
/** Keep a manual selection visible when returning from the full catalog. */
export function instanceChoices(
  options: CatalogOptions,
  showAll: boolean,
  selected: string,
): CatalogOption[] {
  if (showAll || !options.sizes.length) return options.allSizes;
  const current = options.allSizes.find((s) => s.value === selected);
  return current && !options.sizes.some((s) => s.value === selected)
    ? [...options.sizes, current]
    : options.sizes;
}
function restricted(sku: Row, region: string): boolean {
  return rows(sku.restrictions).some((restriction) => {
    // Zone-only restrictions do not prohibit this app's non-zonal deployments.
    if (text(restriction.type).toLowerCase() !== "location") return false;
    const locations = [
      ...strings(restriction.values),
      ...strings(row(restriction.restrictionInfo).locations),
    ];
    return locations.length === 0 || has(locations, region);
  });
}

/** Catalog options are not a capacity or quota guarantee; plan/apply must revalidate. */
export function catalogOptions(
  provider: string,
  raw: unknown,
  selectedRegion: string,
  requirements: { players: number; mods: string } = {
    players: 6,
    mods: "light",
  },
): CatalogOptions {
  const result = (regions: CatalogOption[], sizes: CatalogOption[]) => ({
    regions,
    sizes: recommended(sizes, requirements.players, requirements.mods),
    allSizes: sizes,
  });
  const catalog = row(raw);
  const regions = rows(catalog.regions);
  const sizes = rows(catalog.sizes);
  if (provider === "azure") {
    return result(
      unique(
        regions
          .filter(
            (r) => text(row(r.metadata).regionType).toLowerCase() !== "logical",
          )
          .map((r) => option(r.name, r.displayName)),
      ),
      unique(
        sizes
          .filter((s) => {
            const capabilities = rows(s.capabilities);
            const architecture = text(
              capabilities.find((c) => c.name === "CpuArchitectureType")?.value,
            ).toLowerCase();
            return (
              selectedRegion !== "" &&
              s.resourceType === "virtualMachines" &&
              /^Standard_[DF]\d/i.test(text(s.name)) &&
              architecture === "x64" &&
              has(s.locations, selectedRegion) &&
              !restricted(s, selectedRegion)
            );
          })
          .map((s) => {
            const cap = (name: string) =>
              rows(s.capabilities).find((c) => c.name === name)?.value;
            return size(s.name, cap("vCPUs"), cap("MemoryGB"));
          }),
      ),
    );
  }
  if (provider === "digital_ocean") {
    return result(
      unique(
        regions
          .filter((r) => r.available === true)
          .map((r) => option(r.slug, r.name)),
      ),
      unique(
        sizes
          .filter(
            (s) =>
              selectedRegion !== "" &&
              s.available === true &&
              has(s.regions, selectedRegion) &&
              !row(s.gpu_info).count &&
              !text(s.slug).startsWith("gpu-") &&
              !text(s.architecture).toLowerCase().includes("arm"),
          )
          .map((s) =>
            size(
              s.slug,
              s.vcpus,
              s.memory,
              1024,
              row(s).price_monthly as number,
              "USD",
            ),
          ),
      ),
    );
  }
  if (provider === "hetzner") {
    return result(
      unique(
        regions.map((r) => option(r.name, r.description || r.city || r.name)),
      ),
      unique(
        sizes
          .filter(
            (s) =>
              selectedRegion !== "" &&
              text(s.architecture).toLowerCase() === "x86" &&
              s.deprecated !== true &&
              (s.locations === undefined ||
                rows(s.locations).some(
                  (l) =>
                    l.name === selectedRegion &&
                    l.available === true &&
                    !l.deprecation,
                )) &&
              rows(s.prices).some((p) => p.location === selectedRegion),
          )
          .map((s) => {
            const price = rows(s.prices).find(
              (p) => p.location === selectedRegion,
            );
            const monthly = row(row(price).price_monthly).net;
            return size(s.name, s.cores, s.memory, 1, number(monthly), "EUR");
          }),
      ),
    );
  }
  if (provider === "vultr") {
    return result(
      unique(
        regions.map((r) =>
          option(
            r.id,
            [text(r.city), text(r.country)].filter(Boolean).join(", "),
          ),
        ),
      ),
      unique(
        sizes
          .filter(
            (s) =>
              selectedRegion !== "" &&
              has(s.locations, selectedRegion) &&
              ["vc2", "vhf", "vhp", "voc"].includes(text(s.type)) &&
              !text(s.architecture).toLowerCase().includes("arm"),
          )
          .map((s) =>
            size(
              s.id,
              s.vcpu_count,
              s.ram,
              1024,
              number(
                row(row(row(s).location_cost)[selectedRegion]).monthly_cost,
              ),
              "USD",
            ),
          ),
      ),
    );
  }
  return result([], []);
}
