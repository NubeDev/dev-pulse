# Projects ↔ rbx-docs alignment

> Mapping every dev-pulse project to the [rbx-docs](https://nubeio.github.io/rbx-docs/)
> site, plus a tag taxonomy so the portfolio can be sliced by docs
> section, product family, and workstream type.
>
> Data sourced live from `https://dev-pulse.fly.dev` on 2026-07-08
> (27 projects: 20 active, 7 archived) cross-referenced against each
> project's linked repos and the rbx-docs sitemap.

---

## 0. The idea (tagging strategy)

rbx-docs has a fixed tree of **6 top-level domains** → products. We
mirror that tree as **tag names** so a project carries the same label
its documentation lives under. Three tag prefixes, each answers a
different question:

| Prefix | Answers | Example |
|---|---|---|
| `docs:<domain>` | *Which rbx-docs section describes this work?* | `docs:rubix-frontend` |
| `product:<family>` | *Which hardware product line is this?* | `product:acx` |
| `category:<type>` | *What kind of work is it?* | `category:firmware` |

Why prefixes and not a flat list: dev-pulse's saved-view engine
supports `tag:<key>` group-by and filtering (see
[MANAGING-VIEWS.md](MANAGING-VIEWS.md)). Prefixes turn the tag list
into a queryable namespace — *"group by `docs:` tags"* gives you a
roll-up of the portfolio by documentation area; *"group by `product:`"*
gives you a per-hardware-line view. A flat bag of words can't do that.

The existing tags already half-use this convention
(`category:hardware`, `category:software`, …). This doc formalises it
and adds the two missing prefixes (`docs:`, `product:`).

---

## 1. Review — current state findings

Before the table, what the data turned up:

1. **Tagging is sparse & inconsistent.** Of 20 active projects, the
   `Product` tag hits 13 and `FY27` hits 19, but the product-family
   tags (`ACB`, `ACBM`, `zone-controller`) are barely used
   (1 / 4 / 2 links). There is **no `docs:` tag yet** — projects can't
   be sliced by documentation area today.
2. **3 active projects have zero linked repos**, making a docs match
   harder to verify: `IO-22-EXPANSION`, `IO-22-BAC+CLOUD-PRG`,
   `ACX-IOB`. Their descriptions are the only signal.
3. **Naming / typo issues:**
   - `Water Value - Water` (archived) — should be `Water Valve`.
   - `ZC Daikin` — terse; the rbx-docs page is *Zone Controller ·
     Daikin P1P2*.
4. **Superseded duplicates:** archived `TMV` is the predecessor of
   active `TMVM v2.0 (OEM)` — both point at the same docs page.
   Archived LoRa projects (`LoRa Water Valve`, `Leak sensor (LoRa)`,
   `IAG-Water Flow device`) were folded into `Insurance Project (PoC)`
   but still exist as separate rows.
5. **Good news:** every active project *does* have a sensible home in
   rbx-docs — no orphans. The mapping is high-confidence for ~16/20.

---

## 2. Proposed tag taxonomy

### 2.1 `docs:` — rbx-docs domain (new)

| Tag | rbx-docs section | Use for |
|---|---|---|
| `docs:easy-add` | Easy Add | Scan-to-commission UX, onboarding flows |
| `docs:rubix` | Rubix (top) | Cross-cutting Rubix platform work |
| `docs:rubix-backend` | Rubix › Backend | Workspace, Flow, Rules, Datasources, Extensions |
| `docs:rubix-frontend` | Rubix › Frontend | Shell, Dashboard, Workspaces, Settings |
| `docs:riot` | Riot | rp-core node-graph execution engine |
| `docs:riot-runtime` | Riot Runtime | ESP32 firmware foundation (ACBM, ACX, IO-22) |
| `docs:control-engine` | Control Engine | C++20 distributed dataflow framework |
| `docs:product` | Products | Hardware reference / bring-up |

### 2.2 `product:` — hardware family (new)

| Tag | rbx-docs product page |
|---|---|
| `product:acbl` | [Gateways › ACB-L](https://nubeio.github.io/rbx-docs/products/gateways/acbl/overview/) |
| `product:acbm` | [Gateways › ACB-M](https://nubeio.github.io/rbx-docs/products/gateways/acbm/overview/) |
| `product:acbm-home` | [Gateways › ACB-M Home Gateway](https://nubeio.github.io/rbx-docs/products/gateways/acbm-home-gateway/overview/) |
| `product:acx` | [ACX](https://nubeio.github.io/rbx-docs/products/acx/overview/) |
| `product:io-22` | [IO Controllers](https://nubeio.github.io/rbx-docs/products/io-controllers/io-22-uiuo/overview/) |
| `product:tmv` | [OEM › TMV](https://nubeio.github.io/rbx-docs/products/oem/tmv/overview/) |
| `product:zone-controller` | [OEM › Zone Controller](https://nubeio.github.io/rbx-docs/products/oem/zone-controller/overview/) |
| `product:fga-uart` | [OEM › FGA UART](https://nubeio.github.io/rbx-docs/products/oem/fga-uart/overview/) |
| `product:lora` | [LoRa](https://nubeio.github.io/rbx-docs/products/lora/overview/) |
| `product:micro-edge` | [LoRa › Micro Edge](https://nubeio.github.io/rbx-docs/products/lora/micro-edge/overview/) |

### 2.3 `category:` — workstream type (existing, keep)

Already in use: `category:hardware`, `category:software`,
`category:firmware`, `category:manufacturing`, `category:gtm-go-to-market`,
`category:operations`, `category:compliance`, `category:testing`.

### 2.4 Meta tags (existing, keep)

`FY27`, `FY28`, `PoC`, `Product`, `on-hold`, `test`, `dashboard`.

---

## 3. Master projects table

Confidence: **H**igh (repo name or description is an exact match to a
docs page), **M**edium (strong inference, one signal), **L**ow
(guessed — needs a human to confirm).

### 3.1 Active

| Project | Repos | rbx-docs match | Tags to apply | Conf |
|---|---|---|---|:---:|
| [ACBL](https://nubeio.github.io/rbx-docs/products/gateways/acbl/overview/) | `acbl` | Products › Gateways › ACB-L | `product:acbl` `docs:rubix` `category:hardware` | H |
| [ACX](https://nubeio.github.io/rbx-docs/products/acx/overview/) | `acx` | Products › ACX | `product:acx` `docs:riot-runtime` `category:hardware` | H |
| [ACX-Belimo](https://nubeio.github.io/rbx-docs/products/acx/overview/) | `acx-belimo` | Products › ACX (Belimo actuator integration) | `product:acx` `docs:riot-runtime` `category:firmware` | H |
| [ACX-Canbus](https://nubeio.github.io/rbx-docs/products/acx/overview/) | `acx-canbus` | Products › ACX (CAN-bus variant) | `product:acx` `docs:riot-runtime` `category:firmware` | H |
| [ACX-IOB](https://nubeio.github.io/rbx-docs/products/acx/overview/) | *(none)* | Products › ACX (IO board) | `product:acx` `docs:riot-runtime` `category:hardware` | M |
| [Gen-02 Dashboard (Cloud)](https://nubeio.github.io/rbx-docs/rubix/frontend/dashboard/overview/) | `gen2-cloud`, `rubix` | Rubix › Frontend › Dashboard (cloud) | `docs:rubix-frontend` `category:software` | H |
| [Gen-02 Software](https://nubeio.github.io/rbx-docs/rubix/backend/overview/) | `gen2-software` | Rubix › Backend | `docs:rubix-backend` `category:software` | H |
| [Holi GW](https://nubeio.github.io/rbx-docs/products/gateways/acbm-home-gateway/overview/) | `holi-gw` | Products › Gateways › ACB-M Home Gateway | `product:acbm-home` `product:acbm` `docs:riot-runtime` `category:hardware` | M |
| [Holi solution](https://nubeio.github.io/rbx-docs/products/gateways/acbm-home-gateway/overview/) | `holi-solution` | ACB-M Home Gateway + cloud (solution) | `product:acbm-home` `docs:rubix-frontend` `category:software` | M |
| [IO-22-BAC-PRG](https://nubeio.github.io/rbx-docs/products/io-controllers/io-22-uiuo/overview/) | `io22` | Products › IO Controllers (BACnet programming) | `product:io-22` `docs:riot-runtime` `category:firmware` | H |
| [IO-22-BAC+CLOUD-PRG](https://nubeio.github.io/rbx-docs/products/io-controllers/io-22-uiuo/overview/) | *(none)* | Products › IO Controllers (BACnet + cloud prov.) | `product:io-22` `docs:riot-runtime` `category:firmware` | M |
| [IO-22-EXPANSION](https://nubeio.github.io/rbx-docs/products/io-controllers/io-22-uiuo/overview/) | *(none)* | Products › IO Controllers (expansion) | `product:io-22` `docs:riot-runtime` `category:hardware` | M |
| [Insurance Project (PoC)](https://nubeio.github.io/rbx-docs/products/lora/overview/) | `iag-solution` | LoRa (valve + leak + flow) + cloud app | `product:lora` `docs:easy-add` `category:software` `PoC` | M |
| [ME-02](https://nubeio.github.io/rbx-docs/products/lora/micro-edge/overview/) | `me-optical` | Products › LoRa › Micro Edge (optical/pulse/leak) | `product:micro-edge` `product:lora` `docs:riot-runtime` `category:firmware` | M |
| [riot](https://nubeio.github.io/rbx-docs/riot/overview/) | `rp-core` | Riot (rp-core node-graph engine) | `docs:riot` `category:software` | H |
| [Scan to Dashboard](https://nubeio.github.io/rbx-docs/easy-add/overview/) | `scan-to-dashboard` | Easy Add (scan-to-commission) | `docs:easy-add` `category:software` | H |
| [TMVM v2.0 (OEM)](https://nubeio.github.io/rbx-docs/products/oem/tmv/overview/) | `ACX-hardware`, `galvin-tmv` | Products › OEM › TMV (CliniMix Gen 2) | `product:tmv` `docs:product` `category:hardware` | H |
| [UART](https://nubeio.github.io/rbx-docs/products/oem/fga-uart/overview/) | `fga-uart-fw` | Products › OEM › FGA UART | `product:fga-uart` `docs:riot-runtime` `category:firmware` | H |
| [ZC Daikin](https://nubeio.github.io/rbx-docs/products/oem/zone-controller/daikin-p1p2/overview/) | `zc-daikin` | Products › OEM › Zone Controller › Daikin P1P2 | `product:zone-controller` `docs:riot-runtime` `category:firmware` | H |
| [Zoneconnex V2](https://nubeio.github.io/rbx-docs/products/oem/zone-controller/overview/) | `zoneconnex-2` | Products › OEM › Zone Controller | `product:zone-controller` `docs:riot-runtime` `category:hardware` | M |
| ABC-TEST *(test project)* | — | n/a | `test` | — |

### 3.2 Archived (kept for history — tag for searchability)

| Project | rbx-docs match | Tags to apply |
|---|---|---|
| [TMV](https://nubeio.github.io/rbx-docs/products/oem/tmv/overview/) | Products › OEM › TMV *(superseded by TMVM v2.0)* | `product:tmv` `docs:product` |
| [LoRa Water Valve](https://nubeio.github.io/rbx-docs/products/lora/water-valve/overview/) | Products › LoRa › Water Valve | `product:lora` `docs:riot-runtime` |
| [Water Value - Water](https://nubeio.github.io/rbx-docs/products/lora/water-valve/overview/) *(typo: "Value"→"Valve")* | Products › LoRa › Water Valve | `product:lora` `docs:riot-runtime` |
| [Leak sensor (LoRa)](https://nubeio.github.io/rbx-docs/products/lora/water-leak/overview/) | Products › LoRa › Water Leak Sensor | `product:lora` `docs:riot-runtime` |
| [IAG-Water Flow device](https://nubeio.github.io/rbx-docs/products/lora/water-flow-sensor/overview/) | Products › LoRa › Water Flow Sensor | `product:lora` `docs:riot-runtime` |
| [IAG - APP](https://nubeio.github.io/rbx-docs/products/lora/overview/) | LoRa solution app *(folded into Insurance Project)* | `product:lora` `docs:easy-add` |

---

## 4. Gaps & actions for you

| # | Action | Why |
|---|---|---|
| 1 | **Confirm the 3 no-repo projects** (`ACX-IOB`, `IO-22-EXPANSION`, `IO-22-BAC+CLOUD-PRG`) and link their repos | Removes the M-confidence guesses; a repo name is the most reliable docs-match signal |
| 2 | **Confirm `Holi GW` / `Holi solution` = ACB-M Home Gateway** | The "Holi" naming is internal jargon; I'm 70% on the home-gateway mapping |
| 3 | **Rename `Water Value - Water` → `Water Valve (LoRa)`** before re-tagging, or just leave archived | Typo makes it unsearchable |
| 4 | **Decide:** keep superseded archived dupes (`TMV`, `LoRa Water Valve`) or hard-hide them | They clutter tag roll-ups; archiving already excludes them from active views |
| 5 | **Apply the tags** — see §5 for a script, or do it via the UI tag panel | This is the actual "align to docs" step |

---

## 5. Applying the tags (API)

Tags are created once (`POST /tags`) then linked to projects in a
batch (`POST /tags/{id}/links`, kind `project`). Auth + CSRF as in
[READING-PROJECTS.md §1](READING-PROJECTS.md#1-auth-recap). All tags
should be **`org`-scoped** so the whole team sees them.

```bash
HOST=https://dev-pulse.fly.dev
COOKIES=/tmp/dp-cookies.txt
ORG_ID=4d76fd47-cfa6-4187-8119-fcf9b3bd4b6a   # NubeIO

# 1. login (reuse cookie jar)
CSRF=$(curl -sS -c "$COOKIES" -X POST "$HOST/auth/login" \
  -H 'Content-Type: application/json' \
  -d '{"email":"dev@dev.com","password":"<secret>"}' \
  | python3 -c "import json,sys; print(json.load(sys.stdin)['csrf_token'])")

# 2. create one docs tag (repeat per tag in §2.1 / §2.2)
TAG_ID=$(curl -sS -b "$COOKIES" -X POST "$HOST/tags" \
  -H 'Content-Type: application/json' -H "x-csrf-token: $CSRF" \
  -d "{\"scope_kind\":\"org\",\"scope_id\":\"$ORG_ID\",\"name\":\"docs:rubix-frontend\",\"color\":\"#6366f1\"}" \
  | python3 -c "import json,sys; print(json.load(sys.stdin)['id'])")

# 3. link it to every project that should carry it (transactional batch)
curl -sS -b "$COOKIES" -X POST "$HOST/tags/$TAG_ID/links" \
  -H 'Content-Type: application/json' -H "x-csrf-token: $CSRF" \
  -d '{"items":[{"kind":"project","target_id":"<project-uuid>"}]}'
```

`POST /tags/{id}/links` is **all-or-nothing per batch** — any bad
target rejects the whole call with a per-item error array, so nothing
half-applies. See [tags.rs §7.5](../../crates/dp-rest/src/tags.rs) for
the full contract.

---

## 6. Source of truth

- rbx-docs tree: <https://nubeio.github.io/rbx-docs/> (sitemap, 2026-07-08)
- Project + repo data: live `GET /projects`, `GET /projects/{id}/repos`
- Tag model: `crates/dp-rest/src/tags.rs`, [SCOPE-PROJECTS.md §7](../../SCOPE-PROJECTS.md)

Re-check the rbx-docs sitemap whenever new product pages land — the
`docs:` / `product:` tag set should mirror it one-for-one.
