# Desktop OSS UI presentation

## Purpose and boundary

The desktop Vite build alone sets `BIFROST_DESKTOP_HIDE_ENTERPRISE_UI=true`. Vite defaults it to `false`; `HIDE_ENTERPRISE_UI = !IS_ENTERPRISE && flag` ensures an Enterprise build retains its normal UI. This is compile-time, presentation-only behavior—not a gateway setting, entitlement decision, API change, backend feature change, route removal, alias change, or persisted configuration.

## Desktop checklist

- [x] Scope the exact environment variable to the desktop Vite invocation and restore its previous process value, including an absent value.
- [x] Keep ordinary Vite builds default-off.
- [x] Hide only audited placeholder navigation from sidebar/search/keyboard derivation while preserving RBAC evaluation.
- [x] Retain Teams, Customers, Virtual Keys, OSS navigation, branding, help, theme, onboarding, restart, and release cards.
- [x] Hide only the audited connector selector buttons; preserve direct query routes and plugin renderers.
- [x] Replace generic desktop placeholder marketing with a neutral unavailable state.
- [x] Retain API Keys OSS authentication guidance while omitting its promotional subsection.
- [x] Suppress only the prompt deployment fallback accordion after unconditional prompt-context use.
- [x] Filter only `enterprise_only` feature-flag rows without mutating server data or toggle behavior.

## Audited scope

| Surface | Hidden in desktop OSS builds | Preserved |
| --- | --- | --- |
| Navigation | Circuit Breaker; Alerting (Channels, Rules, History); Users; Business Units; User Provisioning/SCIM; RBAC; Access Profiles; Projects; Audit Logs; Guardrails; Edge Control; Cluster Config; Adaptive Routing | Dashboard, logs, providers, model catalog, budgets, model limits, ordinary routing, Complexity Router, MCP, plugins, webhooks, Virtual Keys, Teams, Customers, prompt/skills repositories and OSS settings |
| Connectors | Datadog, BigQuery, Kafka, Pub/Sub and Splunk selectors | OpenTelemetry, Prometheus, Maxim, the existing disabled New Relic coming-soon card and all plugin renderers/routes |
| Promotions | Production-setup demo card; API Keys granular-scope upsell; Prompt Deployments accordion | Onboarding, restart/update notices, Bifrost attribution, help, theme, API Keys basic-auth guidance, prompt editing and model parameters |
| Feature Flags | Rows explicitly marked `enterprise_only` by the API | All other rows, API response, permissions and mutations |
| Old direct links | Generic Enterprise marketing content | Existing routes show a neutral unavailable message; backend endpoints remain unchanged |

The `@enterprise` import prefix is not an exclusion rule: its OSS fallbacks also
contain working Teams, Customers and authentication UI. New upstream items must
be audited individually, never hidden solely by their directory or name.

## Modified source and reason

| File | Reason |
| --- | --- |
| `desktop/scripts/build.ps1` | Enables the UI-only flag exclusively for desktop Vite builds and restores the caller environment. |
| `ui/vite.config.mts` | Defines the exact flag with a false default. |
| `ui/lib/constants/config.ts` | Centralizes the safe `HIDE_ENTERPRISE_UI` predicate. |
| `ui/components/sidebar.tsx` | Tags only audited placeholder entries and filters them before search and keyboard navigation; suppresses only the production-setup demo card. |
| `ui/app/workspace/observability/views/observabilityView.tsx` | Filters only the audited connector buttons. |
| `ui/app/_fallbacks/enterprise/components/views/contactUsView.tsx` | Provides a neutral direct-route fallback without marketing links. |
| `ui/app/_fallbacks/enterprise/components/api-keys/apiKeysIndexView.tsx` | Leaves auth instructions functional and removes only the promotion. |
| `ui/components/prompts/fragments/settingsPanel.tsx` | Omits only the deployment accordion in desktop presentation mode. |
| `ui/app/workspace/config/views/featureFlagsView.tsx` | Filters only Enterprise-only display rows. |

## Upstream synchronization and removal

These marked `bif-app` changes are intentionally small upstream-UI deviations needed because the Tauri host serves the normal Bifrost bundle. On each `upstream/dev` merge, review whether upstream offers an equivalent UI presentation API. If it does, use that API, remove the redundant fork conditions, and validate both normal server and desktop bundles. Do not turn this into a runtime/business configuration or modify Enterprise implementations.

## Validation commands

From the repository root on Windows:

```powershell
# Build the UI and sidecar from this checkout. Includes UI typechecking.
./desktop/scripts/build.ps1 sidecar
npm ci --prefix tests/e2e --no-audit --no-fund

# Build and test both production UI variants against a disposable local gateway.
node desktop/scripts/oss-ui-smoke.mjs

# Existing desktop lifecycle, settings, credential and real gateway checks.
./desktop/scripts/build.ps1 test
```

The UI runner uses the existing locked E2E dependencies and installed Chrome
(`PLAYWRIGHT_CHANNEL` can select another installed channel). It starts its own
sidecar with an OS-generated temporary data directory and random encryption key,
proxies the two isolated production bundles to that process, then closes it and
removes that temporary directory. It does not start the desktop host, use the
user's Credential Manager entry or contact a paid provider. Feature-flag response
fixtures are read-only browser mocks; other tested pages use the actual gateway.

The ordinary run leaves the desktop presentation flag off; the desktop run enables
it. Both bundles are under ignored `desktop/.tools/` directories and do not replace
the canonical embedded UI. Failure screenshots and traces are under
`desktop/test-results/oss-ui-*`. To rerun one mode use `--mode ordinary` or
`--mode desktop`. Windows CI runs both modes before publishing its portable
artifact and uploads the UI test evidence.

For scoped formatting, use the repository's installed
`ui/node_modules/.bin/oxfmt` on the changed TypeScript files; avoid unrelated
format churn. `npm run typecheck --prefix ui` checks the full UI. The upstream
`npm run build --prefix ui` also runs a POSIX `rm/cp` copy step; on Windows the
desktop build script uses the same Vite/typecheck steps and native file copying.
