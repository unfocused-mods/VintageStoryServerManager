# Hosting comparison data

`GET /api/v1/hosting/catalog` is read-only and requires neither a Manager login nor a cloud account. It returns planning offers, not deployable resource plans or capacity guarantees. Provider calls have a 30-second total deadline, successful data is cached for 15 minutes, and partial results for one minute. The UI selects suitable memory/CPU options from player count and mod usage and filters by player region. It does not label geographical proximity as measured latency.

- Azure uses the unauthenticated [Retail Prices API](https://learn.microsoft.com/en-us/rest/api/cost-management/retail-prices/azure-retail-prices) for selected regular Linux x64 D/F sizes. Full-time compute uses 730 hours. Disk, IP and traffic remain explicitly additional; these are not presented as complete totals.
- Vultr uses public plans and regions endpoints, including per-location price overrides. Included bandwidth is displayed in GB. Published plan-region listings are not real-time capacity guarantees.
- DigitalOcean uses a dated snapshot checked 2026-09-06 against [Droplet pricing](https://www.digitalocean.com/pricing/droplets): Basic Regular 2 vCPU/4 GiB for USD 24, 4 vCPU/8 GiB for USD 48 and 8 vCPU/16 GiB for USD 96, including storage, primary IPv4 and bundled transfer. Regional availability is checked after account setup.
- Hetzner uses a dated snapshot checked 2026-09-06 against the [price adjustment table](https://docs.hetzner.com/general/infrastructure-and-availability/price-adjustment/): CX23 EUR 5.49, CX33 EUR 8.49 and CX43 EUR 15.99, plus EUR 0.50 primary IPv4. [Size specifications](https://www.hetzner.com/cloud/cost-optimized/) and [network options](https://docs.hetzner.com/cloud/servers/overview/#network-options) provide resources and IPv4 pricing. These Germany/Finland options may lack current capacity.

Snapshot dates never advance on refresh. Tax, account discounts, credits, optional backups and traffic beyond bundled allowances are excluded. Account-specific catalogs and quotes are revalidated by deployment planning and application.

Manual network verification: `cargo test -p vsm live_public_catalogs --lib -- --ignored --nocapture`. Ordinary tests never depend on live provider services.

## Currencies and running hours

The catalog includes an `fx` object with EUR-base rates for USD, AUD and NZD from the [European Central Bank daily reference feed](https://www.ecb.europa.eu/stats/eurofxref/eurofxref-daily.xml). The ECB usually publishes on working days around 16:00 CET; reference rates are indicative, not transaction quotes. The response keeps the source reference date (`as_of`) separate from retrieval time (`fetched_at`). Conversion is `amount × target rate ÷ source rate`, so both EUR and USD provider quotes convert correctly.

FX requests run alongside provider discovery, have a ten-second deadline and a 64 KiB response limit, and cache successful rates for one hour. Weekends and holidays retain the latest available business-day rate. Dates older than seven days are marked stale; future dates and missing, duplicate, zero or nonfinite rates are rejected. Refresh failures retain previously fetched values only as explicitly stale data and retry after one minute. Unavailable or stale rates leave prices in the provider's original currency rather than assuming parity.

Average running hours range from 0 to 24 per day. Only Azure compute estimates scale by `hours ÷ 24`, assuming the VM is deallocated for the remaining time. Persistent disk and IP costs remain. Hetzner, DigitalOcean and Vultr bundled prices stay unchanged because merely stopping those VMs does not stop their billing. Selecting fewer hours changes an estimate, not a server schedule.

Manual FX verification: `cargo test -p vsm live_ecb_rates --lib -- --ignored --nocapture`.
