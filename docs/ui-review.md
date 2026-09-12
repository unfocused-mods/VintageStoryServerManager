# UI review

## Language and object names

The normal user is the owner of their own installation. Instructions must give them an action they can take, never refer them to an administrator.

- **Manager**: the application that stores the inventory and coordinates operations. Desktop starts a local Manager automatically. A separately hosted Manager is an optional connection.
- **Remote**: the management software installed on a game host. Installing a Remote does not by itself mean the game is ready to join.
- **Game server**: the Vintage Story process, configuration, mods, and world. Use this term where a bare “server” could mean the Manager, host, VM, or game.
- **Host / VM / container**: the machine or container running a Remote and game server. Infrastructure actions name the resource they actually affect.

Account role names may include Administrator in the optional team screen. Role names do not justify instructions such as asking an administrator to complete setup. Empty states should state what is missing and offer the next real action.

## Visual and typography pass

All surface, text, input, and separator greys are achromatic: their red, green, and blue channels are equal. Dark surfaces use `#101010` for the canvas, `#151515` for navigation, `#1b1b1b` for panels and dialogs, and `#252525` for hover/alternate panels. The dialog overlay is black; it cannot introduce a cool tint. Light surfaces use the same neutral approach.

Color is reserved for status and consequential actions. Error text has separate light/dark colors; error panels retain neutral backgrounds. Focus outlines use the theme text color. Input borders have a stronger contrast token than decorative separators.

The application bundles unmodified **IBM Plex Sans** (400, 500, 600) and **IBM Plex Mono** (400), licensed under SIL OFL 1.1. Fonts load from the application, without external font requests. Package versions, licenses, and SHA-256 hashes are in `apps/ui/public/fonts/README.md`. The source is [IBM's official Plex repository](https://github.com/IBM/plex).

The scale is compact: 14px body, 13px controls, 12px secondary labels, 15px small section titles, 17px section/dialog titles, and 20px page titles. No 11px labels remain. Numeric values use tabular figures; paths, code, and console output use the matching monospace family. No decorative imagery is introduced.

Calculated contrast checks:

| Pair | Ratio |
| --- | ---: |
| Dark secondary text / alternate panel | 6.45:1 |
| Light secondary text / sidebar | 5.04:1 |
| Dark input border / input | 3.07:1 |
| Light input border / input | 3.69:1 |
| Dark error text / panel | 7.54:1 |
| Light error text / panel | 6.88:1 |

These checks validate the listed color pairs; they are not a complete accessibility audit.

## Review criteria

- No promotional metrics, slogans, decorative provider strip, or artwork.
- Local desktop startup opens its Manager automatically and exposes a retry on failure.
- First setup offers a deployment target before requiring a cloud account.
- SSH installation distinguishes host verification, Remote installation, game installation, and game startup.
- Docker targets do not expose VM resize, billing, or cloud account controls.
- Connection settings identify a Manager, never an ambiguously named server or controller.
- Queued and incomplete installation states do not expose a false ready-to-join address.
- Billing, compatibility, restore, and destructive-action information stays next to the relevant action.
- Optional multiuser controls do not dominate the owner’s normal flow.

## Remaining review boundaries

The typography and palette changes passed source review and a production build. All 24 UI tests passed, including deployment-target and Remote setup coverage. The integration owner inspected the running browser UI in light and dark themes, including the target picker, Remote setup, and local Docker dialogs. Browser autofill was corrected to preserve neutral input surfaces. Full keyboard, screen-reader, narrow-mobile, and packaged-desktop audits remain separate release checks.

SSH fingerprint comparison remains necessary and should point to the cloud console or host terminal. A UI must not silently accept an unknown host key. Docker availability, image loading, container permissions, and host storage failures need actionable errors in the owning setup workflow. Advanced backup credential setup and team TOTP enrollment remain usability work where a guided path has not yet been implemented.
