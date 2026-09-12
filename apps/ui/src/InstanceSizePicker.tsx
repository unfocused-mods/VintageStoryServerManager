import { memoryEstimate } from "./sizing";
import { type CatalogOptions, instanceChoices } from "./catalog";
const tierNames = {
  economy: "Economy",
  recommended: "Recommended",
  headroom: "More headroom",
};
export default function InstanceSizePicker({
  options,
  value,
  onChange,
  showAll,
  onShowAll,
  loading = false,
  prices = {},
  pricingLoading = false,
  priceErrors = [],
  players = 6,
  mods = "light",
  emptyMessage,
}: {
  options: CatalogOptions;
  value: string;
  onChange: (value: string) => void;
  showAll: boolean;
  onShowAll: (value: boolean) => void;
  loading?: boolean;
  prices?: Record<string, { price: number; currency: string }>;
  pricingLoading?: boolean;
  priceErrors?: string[];
  players?: number;
  mods?: string;
  emptyMessage?: string;
}) {
  const choices = instanceChoices(options, showAll, value);
  const selected = options.allSizes.find((s) => s.value === value);
  return (
    <div className="form">
      <label className="field">
        <span>Instance size</span>
        <select
          value={value}
          onChange={(e) => onChange(e.target.value)}
          disabled={loading}
          required
        >
          <option value="">
            {loading ? "Loading sizes…" : "Select a size"}
          </option>
          {choices.map((s) => {
            const quote = prices[s.value];
            const tier =
              !showAll && s.recommendation
                ? `${tierNames[s.recommendation]} — `
                : !showAll && s.value === value && !s.recommendation
                  ? "Selected — "
                  : "";
            const price =
              s.monthlyPrice !== undefined
                ? ""
                : quote
                  ? ` · ${quote.currency} ${quote.price.toFixed(2)}/mo`
                  : priceErrors.includes(s.value)
                    ? " · Price unavailable"
                    : pricingLoading && (s.recommendation || s.value === value)
                      ? " · Loading price…"
                      : " · Price checked at review";
            return (
              <option key={s.value} value={s.value}>
                {tier}
                {s.label}
                {price}
              </option>
            );
          })}
        </select>
      </label>
      {!loading && options.allSizes.length === 0 && emptyMessage && (
        <p className="muted small" role="status">
          {emptyMessage}
        </p>
      )}
      {options.allSizes.length > options.sizes.length &&
        options.sizes.length > 0 && (
          <button
            type="button"
            className="text-button"
            onClick={() => onShowAll(!showAll)}
          >
            {showAll
              ? "Show suggested sizes"
              : `Show all ${options.allSizes.length} sizes`}
          </button>
        )}
      {!showAll && options.sizes.length > 0 && (
        <p className="muted small">
          Suggestions for {players} players
          {mods === "none"
            ? " without mods"
            : mods === "heavy"
              ? " with heavy mods"
              : " with light mods"}
          .
        </p>
      )}
      {selected?.vcpus !== undefined && selected.vcpus < 4 && (
        <p className="muted small">
          This size is below Vintage Story’s recommendation of 4 CPU threads.
        </p>
      )}
      {selected?.memoryGiB !== undefined &&
        selected.memoryGiB < memoryEstimate(players, mods) && (
          <p className="muted small">
            This size has less than the estimated{" "}
            {memoryEstimate(players, mods).toFixed(1)} GiB needed for your
            players and mods.
          </p>
        )}
    </div>
  );
}
