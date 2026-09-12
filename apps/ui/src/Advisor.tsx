import { memoryEstimate } from "./sizing";
import { Fragment, useEffect, useMemo, useState } from "react";
import { Loader2, RefreshCw } from "lucide-react";
import { Api, providerNames, type Provider } from "./api";
export interface HostingOffer {
  id: string;
  provider: Provider;
  region: string;
  area: string;
  size: string;
  vcpus: number;
  memory_gib: number;
  compute: number;
  total: number | null;
  currency: string;
  source: string;
  checked_at: string;
  live: boolean;
  shared_cpu: boolean;
  details: string;
}
export interface ExchangeRates {
  base: string;
  rates: Record<string, number>;
  as_of: string | null;
  fetched_at: string;
  source: string;
  status: "current" | "stale" | "unavailable";
  error?: string;
}
type Catalog = {
  offers: HostingOffer[];
  notices: string[];
  fx?: ExchangeRates;
};
export function monthlyAmount(offer: HostingOffer, hours: number): number {
  const compute =
    offer.provider === "azure"
      ? (offer.compute * Math.max(0, Math.min(24, hours))) / 24
      : offer.compute;
  return offer.total === null ? compute : offer.total - offer.compute + compute;
}
export function convertedEstimate(
  offer: HostingOffer,
  hours: number,
  currency: string,
  fx?: ExchangeRates,
): number | null {
  const amount = monthlyAmount(offer, hours);
  if (!Number.isFinite(amount) || amount < 0) return null;
  if (offer.currency === currency) return amount;
  if (fx?.status !== "current") return null;
  const from = fx.rates[offer.currency],
    to = fx.rates[currency];
  if (!Number.isFinite(from) || !Number.isFinite(to) || from <= 0 || to <= 0)
    return null;
  return (amount * to) / from;
}
const areas = [
  ["", "Players in several regions"],
  ["europe", "Europe"],
  ["north_america", "North America"],
  ["south_america", "South America"],
  ["asia", "Asia"],
  ["oceania", "Australia / New Zealand"],
  ["africa", "Africa"],
];
export { memoryEstimate } from "./sizing";

export function suitableOffers(
  offers: HostingOffer[],
  area: string,
  players: number,
  mods: string,
  economy = false,
): HostingOffer[] {
  const memory = memoryEstimate(players, mods);
  return offers
    .filter(
      (o) =>
        (!area || o.area === area) &&
        o.vcpus >= (economy ? 2 : 4) &&
        o.memory_gib >= memory,
    )
    .sort(
      (a, b) =>
        a.currency.localeCompare(b.currency) ||
        (a.total ?? a.compute) - (b.total ?? b.compute),
    );
}
// Input is price-ordered. Keep the cheapest equivalent capacity, then show
// a useful range within the requested workload, rather than the largest VM sold.
export function sizeShortlist(offers: HostingOffer[], players = 6, mods = "light", economy = false): HostingOffer[] {
  const regions = new Map<string, HostingOffer[]>();
  for (const offer of offers) {
    const key = `${offer.provider}:${offer.region}`;
    const rows = regions.get(key) || [];
    if (!rows.some((row) => row.vcpus === offer.vcpus && row.memory_gib === offer.memory_gib && row.shared_cpu === offer.shared_cpu)) rows.push(offer);
    regions.set(key, rows);
  }
  const selected = new Set<string>();
  for (const rows of regions.values()) {
    const cheapest = rows[0];
    selected.add(cheapest.id);
    const memoryLimit = Math.max(memoryEstimate(players, mods) * 2, Math.min(...rows.map((row) => row.memory_gib)) * 2);
    const cpuLimit = Math.max((economy ? 2 : 4) * 2, Math.min(...rows.map((row) => row.vcpus)));
    const upgrades = rows.filter((row) => row.id !== cheapest.id && row.memory_gib <= memoryLimit && row.vcpus <= cpuLimit)
      .sort((a, b) => a.memory_gib - b.memory_gib || a.vcpus - b.vcpus);
    if (upgrades.length) {
      selected.add(upgrades[0].id);
      selected.add(upgrades[upgrades.length - 1].id);
    }
  }
  return offers.filter((offer) => selected.has(offer.id));
}
export default function Advisor({
  api,
  onChoose,
}: {
  api: Api;
  onChoose?: (provider: Provider) => void;
}) {
  const [catalog, setCatalog] = useState<Catalog | null>(null),
    [players, setPlayers] = useState(6),
    [mods, setMods] = useState("light"),
    [economy, setEconomy] = useState(false),
    [area, setArea] = useState(""),
    [budget, setBudget] = useState(""),
    [currency, setCurrency] = useState("USD"),
    [hours, setHours] = useState(24),
    [busy, setBusy] = useState(false),
    [error, setError] = useState(""),
    [expanded, setExpanded] = useState<string | null>(null),
    [showAll, setShowAll] = useState<Record<string, boolean>>({}),
    [selectedRegions, setSelectedRegions] = useState<Record<string, string>>(
      {},
    );
  const load = async () => {
    setBusy(true);
    setError("");
    try {
      setCatalog(await api.request<Catalog>("/hosting/catalog"));
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  };
  useEffect(() => {
    void load();
  }, [api.url]);
  const offers = useMemo(
    () =>
      suitableOffers(catalog?.offers || [], area, players, mods, economy).sort(
        (a, b) =>
          (convertedEstimate(a, hours, currency, catalog?.fx) ?? Infinity) -
          (convertedEstimate(b, hours, currency, catalog?.fx) ?? Infinity),
      ),
    [catalog, area, players, mods, economy, hours, currency],
  );
  const shown = offers;
  const money = (value: number, code: string) =>
    new Intl.NumberFormat(undefined, {
      style: "currency",
      currency: code,
      currencyDisplay: "code",
    }).format(value);
  return (
    <div className="form">
      <p>Compare hosting before opening a cloud account.</p>
      <div className="form-grid">
        <label className="field">
          <span>Players online at once</span>
          <input
            type="number"
            min={1}
            max={200}
            value={players}
            onChange={(e) => setPlayers(Math.max(1, Number(e.target.value)))}
          />
        </label>
        <label className="field">
          <span>Mods</span>
          <select value={mods} onChange={(e) => setMods(e.target.value)}>
            <option value="none">None</option>
            <option value="light">A few mods</option>
            <option value="heavy">Heavily modded</option>
          </select>
        </label>
        <label className="field">
          <span>Where are your players?</span>
          <select value={area} onChange={(e) => setArea(e.target.value)}>
            {areas.map(([id, label]) => (
              <option key={id} value={id}>
                {label}
              </option>
            ))}
          </select>
        </label>
        <label className="field">
          <span>Server sizing</span>
          <select
            value={economy ? "economy" : "recommended"}
            onChange={(e) => setEconomy(e.target.value === "economy")}
          >
            <option value="recommended">Recommended — 4+ vCPU</option>
            <option value="economy">Economy — allow 2 vCPU</option>
          </select>
        </label>
        <label className="field">
          <span>Currency</span>
          <select
            value={currency}
            onChange={(e) => setCurrency(e.target.value)}
          >
            {["USD", "EUR", "NZD", "AUD"].map((code) => (
              <option key={code}>{code}</option>
            ))}
          </select>
        </label>
        <label className="field">
          <span>Hours on per day</span>
          <input
            type="number"
            min={0}
            max={24}
            step={0.5}
            value={hours}
            onChange={(e) =>
              setHours(Math.max(0, Math.min(24, Number(e.target.value) || 0)))
            }
          />
        </label>
        <label className="field">
          <span>Monthly budget ({currency}, optional)</span>
          <input
            aria-label="Monthly budget"
            type="number"
            min={0}
            value={budget}
            onChange={(e) => setBudget(e.target.value)}
            placeholder="Any budget"
          />
        </label>
      </div>
      {hours < 24 && (
        <p className="muted small">
          Hours change estimates only. Azure assumes the VM is deallocated when
          off.
        </p>
      )}
      {economy && (
        <p className="muted small">
          Economy includes 2-vCPU servers, below Vintage Story’s recommendation
          of 4 threads. Busy worlds may run more slowly.
        </p>
      )}
      <details className="small">
        <summary>
          Estimated memory: {memoryEstimate(players, mods).toFixed(1)} GiB
        </summary>
        <p>
          <a
            href="https://www.vintagestory.at/selfhosting/"
            target="_blank"
            rel="noreferrer"
          >
            Vintage Story’s guidance
          </a>{" "}
          is 1 GB plus 0.3 GB per player. We add 0.5 GiB for the operating
          system and agent, plus{" "}
          {mods === "heavy" ? "2" : mods === "light" ? "0.5" : "0"} GiB for
          mods. Mod needs vary.
        </p>
      </details>
      <p className="muted small">
        Sizes are selected for your player count and mods. Nearby regions are a
        starting point; latency has not been measured. Prices exclude taxes,
        discounts and credits.
      </p>
      {error && (
        <p className="inline-error" role="alert">
          {error}
        </p>
      )}
      {busy && (
        <p role="status">
          <Loader2 size={16} className="spin" /> Loading hosting options…
        </p>
      )}
      {catalog &&
        shown.some(
          (o) =>
            o.currency !== currency &&
            convertedEstimate(o, hours, currency, catalog.fx) === null,
        ) && (
          <p className="muted small" role="status">
            {catalog.fx?.status === "stale"
              ? "Exchange rates are out of date."
              : "Exchange rates are unavailable."}{" "}
            Original prices are shown where conversion is unavailable.
          </p>
        )}
      {catalog?.fx?.status === "current" &&
        shown.some((o) => o.currency !== currency) && (
          <p className="muted small">
            <a href={catalog.fx.source} target="_blank" rel="noreferrer">
              ECB exchange rates
            </a>{" "}
            - {catalog.fx.as_of}
          </p>
        )}
      {catalog?.notices.map((n) => (
        <p className="muted small" key={n}>
          {n}
        </p>
      ))}
      {catalog && !shown.length && (
        <p>
          No suitable options in this region. Try a nearby region or fewer
          players.
        </p>
      )}
      {!!shown.length && (
        <div className="hosting-table-scroll">
          <table className="hosting-table">
            <thead>
              <tr>
                <th>Provider</th>
                <th>Location & resources</th>
                <th>Monthly estimate - {currency}</th>
                <th>
                  <span className="sr-only">Setup</span>
                </th>
              </tr>
            </thead>
            <tbody>
              {["azure", "hetzner", "digital_ocean", "vultr"].map(
                (provider) => {
                  const rows = shown.filter((o) => o.provider === provider);
                  if (!rows.length) return null;
                  const o =
                    rows.find((row) => row.id === selectedRegions[provider]) ||
                    rows[0];
                  const isExpanded = expanded === provider;
                  const shortlist = sizeShortlist(rows, players, mods, economy);
                  const options = showAll[provider] ? rows : shortlist.filter((row) => row.id !== o.id).concat(o).sort((a, b) => rows.indexOf(a) - rows.indexOf(b));
                  const estimate = convertedEstimate(
                    o,
                    hours,
                    currency,
                    catalog?.fx,
                  );
                  return (
                    <Fragment key={provider}>
                      <tr>
                        <th scope="row">
                          <strong>{providerNames[provider as Provider]}</strong>
                          <button
                            className="text-button"
                            aria-expanded={isExpanded}
                            aria-controls={`hosting-${provider}`}
                            onClick={() =>
                              setExpanded(isExpanded ? null : provider)
                            }
                          >
                            Sizes & regions
                          </button>
                        </th>
                        <td>
                          {o.region}
                          <small>
                            {o.vcpus} vCPU · {o.memory_gib} GiB RAM
                          </small>
                        </td>
                        <td>
                          <strong>
                            {estimate === null
                              ? `Unavailable in ${currency}`
                              : money(estimate, currency)}
                          </strong>
                          {o.total === null && (
                            <small>+ disk, IP & traffic</small>
                          )}
                          <small>
                            {o.live ? "Live" : "Snapshot"} ·{" "}
                            {new Date(o.checked_at).toLocaleDateString()}
                          </small>
                          {estimate === null && (
                            <small>
                              Original:{" "}
                              {money(monthlyAmount(o, hours), o.currency)}
                            </small>
                          )}
                          {hours < 24 && (
                            <small>
                              {provider === "azure"
                                ? `${hours} h/day; deallocate when off`
                                : "Still billed while off"}
                            </small>
                          )}
                          {budget &&
                            estimate !== null &&
                            estimate > Number(budget) && (
                              <small>Above budget</small>
                            )}
                          {budget && estimate === null && (
                            <small>Budget comparison unavailable</small>
                          )}
                        </td>
                        <td>
                          {onChoose && (
                            <button
                              className="button secondary"
                              aria-label={`Set up with ${providerNames[provider as Provider]}`}
                              onClick={() => onChoose(provider as Provider)}
                            >
                              Set up
                            </button>
                          )}
                        </td>
                      </tr>
                      {isExpanded && (
                        <tr id={`hosting-${provider}`}>
                          <td colSpan={4}>
                            <div className="hosting-details">
                              {rows.length > 1 && (
                                <label className="field">
                                  <span>
                                    {providerNames[provider as Provider]} sizes & regions
                                  </span>
                                  <select
                                    value={o.id}
                                    onChange={(e) =>
                                      setSelectedRegions({
                                        ...selectedRegions,
                                        [provider]: e.target.value,
                                      })
                                    }
                                  >
                                    {options.map((row) => (
                                      <option key={row.id} value={row.id}>
                                        {row.region} | {row.vcpus} vCPU | {row.memory_gib} GiB | {row.size} -{" "}
                                        {convertedEstimate(
                                          row,
                                          hours,
                                          currency,
                                          catalog?.fx,
                                        ) === null
                                          ? `conversion unavailable (${money(monthlyAmount(row, hours), row.currency)})`
                                          : money(
                                              convertedEstimate(
                                                row,
                                                hours,
                                                currency,
                                                catalog?.fx,
                                              )!,
                                              currency,
                                            )}
                                        {row.total === null
                                          ? " + other costs"
                                          : ""}
                                      </option>
                                    ))}
                                  </select>
                                </label>
                              )}
                              {shortlist.length < rows.length && (
                                <label className="checkbox">
                                  <input type="checkbox" checked={!!showAll[provider]} onChange={(e) => setShowAll({ ...showAll, [provider]: e.target.checked })} />
                                  Show all {rows.length} suitable {providerNames[provider as Provider]} options
                                </label>
                              )}
                              <p className="small">{o.details}</p>
                              <p className="small muted">
                                {o.size} ·{" "}
                                {o.shared_cpu
                                  ? "Shared CPU"
                                  : "Dedicated CPU allocation"}{" "}
                                - Billed in {o.currency}
                              </p>
                              {provider === "azure" && (
                                <p className="small muted">
                                  Compute uses{" "}
                                  {((730 * hours) / 24).toLocaleString(
                                    undefined,
                                    { maximumFractionDigits: 1 },
                                  )}{" "}
                                  hours per month. Disk and IP charges are not
                                  reduced.
                                </p>
                              )}
                              <a
                                href={o.source}
                                target="_blank"
                                rel="noreferrer"
                              >
                                Pricing source
                              </a>
                            </div>
                          </td>
                        </tr>
                      )}
                    </Fragment>
                  );
                },
              )}
            </tbody>
          </table>
        </div>
      )}
      <p className="muted small">
        Availability and prices are checked again before deployment.
      </p>
      <button
        className="button secondary"
        disabled={busy}
        onClick={() => void load()}
      >
        <RefreshCw size={15} />
        Refresh options
      </button>
    </div>
  );
}
