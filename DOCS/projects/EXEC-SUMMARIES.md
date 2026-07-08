# Executive Summaries — Active Projects

> Structured exec summaries for the 20 active dev-pulse projects, with
> every section sourced from the matching [rbx-docs](https://nubeio.github.io/rbx-docs/)
> page. Section model mirrors the `GET/PATCH /projects/{id}/exec-summary`
> API (summary / scope / requirements / hardware / commercial) so these
> can be pushed straight into the app later.
>
> Sourced 2026-07-08. Where a docs page was a stub, the section is
> marked *(not yet documented)* rather than invented.

---

## How to read each entry

| Section | What it is |
|---|---|
| **Summary** | product_name, part_number, objective, problem, value, differentiators, success_criteria |
| **Scope** | in_scope, out_of_scope, dependencies |
| **Requirements** | must_have, architecture, protocols[], power, mounting |
| **Hardware** | hardware_features, physical_notes |
| **Commercial** | target_market, revenue_model |

---

## 1. ACBL

**dev-pulse:** `68c9a379-c939-49b2-8112-170fdfeb2a0a` · repo `NubeIO/acbl` · tags `product:acbl` `docs:rubix` `category:hardware`
**Source:** [ACB-L Overview](https://nubeio.github.io/rbx-docs/products/gateways/acbl/overview/)

- **Summary**
  - product_name: **ACB-L** (Automation Control Board — Large)
  - objective: Provide a two-PCB Linux gateway combining an ACB-M (ESP32) field controller with a Raspberry Pi CM4 running the full Rubix stack.
  - problem: Building-automation sites need both edge field I/O (Modbus/BACnet/LoRa) and a capable Linux host for the dashboard/control-engine, usually met by separate boxes.
  - value: One stacked board does both — the CM4 runs Rubix + Control Engine while the ACB-M handles Riot Runtime field comms.
  - differentiators: Dual-PCB stack; LoRaRAW 2-way micro gateway; up to 4× IO-22 expansion modules; top-mount 4G/LoRaWAN options.
  - success_criteria: Full Rubix stack on CM4 + Riot field I/O on ACB-M, validated end to end.
- **Scope**
  - in_scope: ACB-M bottom PCB (ESP32: Riot, Modbus RTU/TCP, BACnet MSTP/IP, LoRaRAW), CM4 top PCB (Linux, Control Engine, Dashboard backend), gigabit + 100 Mbps Ethernet, WiFi, onboard graphics.
  - out_of_scope: LoRaWAN gateway hardware (requires top-mount module); ACB-P power-supply variant is a separate config.
  - dependencies: Raspberry Pi CM4 supply; ACB-M bottom board.
- **Requirements**
  - architecture: Two-PCB stack; bottom = ACB-M or ACB-P; top = CM4.
  - protocols: BACnet/IP, BACnet/MSTP, Modbus TCP, Modbus RTU (RS-485), LoRaRAW (2-way), MQTT/REST.
  - power: 24V/DC (via ACB-P/ACBM rail).
- **Hardware**
  - hardware_features: 1× 1000 Mbps + 1× 100 Mbps to CM4; LoRa transceiver; IO expansion (max 4 IO-22 modules right-side); top-mount 4G/LoRaWAN; side plugin expansion.
  - physical_notes: Stacked board form factor; onboard graphics via Rubix.
- **Commercial**
  - target_market: Commercial building automation, HVAC, facilities IoT gateways.
  - revenue_model: Hardware sale (gateway).

---

## 2. ACX

**dev-pulse:** `344929d8-4d4d-421f-9fec-5f4e339f2d4b` · repo `NubeIO/acx` · tags `product:acx` `docs:riot-runtime` `category:hardware`
**Source:** [ACX Overview](https://nubeio.github.io/rbx-docs/products/acx/overview/)

- **Summary**
  - product_name: **ACX**
  - objective: Compact, cost-effective ESP32 controller — a smaller-footprint ACB-M running the same Riot Runtime.
  - problem: Many install points need Riot Runtime control but only a few I/O points; a full ACB-M is overkill.
  - value: Same firmware/protocol set as ACB-M in a smaller package for space/cost-sensitive jobs.
  - differentiators: USB-C + 12V jack power; 1× isolated RS-485; UART/Daikin P1P2; LoRa; WiFi.
  - success_criteria: Riot Runtime + ACB-M-equivalent protocols on the compact board.
- **Scope**
  - in_scope: ESP32 Riot Runtime firmware (esp32-core-lib); 1 DI, 1 RO, 2 UI; RS-485; WiFi; LoRa; UART/Daikin P1P2.
  - out_of_scope: Full ACB-M I/O count; Linux/Rubix host (no CM4).
  - dependencies: esp32-core-lib; rp-core Riot packages.
- **Requirements**
  - architecture: Single ESP32 board; Riot packages compiled with ACX target.
  - protocols: Per Riot Runtime protocols (Modbus, BACnet, LoRaRAW).
  - power: USB-C or 12VDC jack.
- **Hardware**
  - hardware_features: 1× DI (dry contact), 1× RO, 2× UI (0–10V, 10k thermistor, pulse/DI); isolated RS-485; LoRa transceiver; WiFi.
  - physical_notes: Compact form factor vs ACB-M.
- **Commercial**
  - target_market: Zone/room control, small HVAC, OEM integrations needing few I/O points.
  - revenue_model: Hardware sale (controller).

---

## 3. ACX-Belimo

**dev-pulse:** `e3c871a5-bbe0-48d3-a467-38263cd90ace` · repo `NubeIO/acx-belimo` · tags `product:acx` `docs:riot-runtime` `category:firmware`
**Source:** [ACX Overview](https://nubeio.github.io/rbx-docs/products/acx/overview/) (Belimo = actuator/valve driver integration on ACX)

- **Summary**
  - product_name: **ACX-Belimo**
  - objective: ACX firmware variant driving Belimo actuators/valves over their native protocol.
  - problem: Belimo is the dominant HVAC actuator brand; installers need native, reliable drive of those devices from an edge controller.
  - value: Pre-integrated Belimo control from the ACX platform without external converters.
  - differentiators: ACX Riot Runtime + Belimo-specific drive logic; analog/MP-Bus actuator support.
  - success_criteria: Confirmed command/feedback round-trip with reference Belimo actuators.
- **Scope**
  - in_scope: ACX hardware + Riot Runtime; Belimo actuator drive node package.
  - out_of_scope: Non-Belimo actuator brands (separate variants); full BMS supervisory logic.
  - dependencies: ACX board; esp32-core-lib; Belimo reference hardware for validation.
- **Requirements**
  - architecture: ACX ESP32 + Riot node graph with Belimo drive nodes.
  - protocols: Belimo actuator signalling (over UI/RS-485 as applicable); Riot Runtime field protocols.
  - power: USB-C / 12VDC (ACX standard).
- **Hardware**
  - hardware_features: ACX I/O set (1 DI, 1 RO, 2 UI, RS-485) used to drive/sense Belimo actuators.
- **Commercial**
  - target_market: HVAC OEMs standardizing on Belimo actuators.
  - revenue_model: Hardware sale (ACX) + firmware variant.

---

## 4. ACX-Canbus

**dev-pulse:** `ea62095b-dce6-459e-84c7-d17040a46066` · repo `NubeIO/acx-canbus` · tags `product:acx` `docs:riot-runtime` `category:firmware`
**Source:** [ACX Overview](https://nubeio.github.io/rbx-docs/products/acx/overview/) (CAN-bus comms variant)

- **Summary**
  - product_name: **ACX-Canbus**
  - objective: ACX firmware variant adding CAN-bus communication for vehicle/equipment networks.
  - problem: Some OEM equipment exposes telemetry/control over CAN bus, which the base ACX UART/RS-485 set doesn't reach.
  - value: Native CAN-bus integration on the ACX edge controller via a Riot node package.
  - differentiators: ACX Riot Runtime + CAN-bus transport; bridges CAN equipment into Rubix.
  - success_criteria: Validated CAN frame read/write against reference equipment.
- **Scope**
  - in_scope: ACX hardware + CAN-bus driver node package.
  - out_of_scope: Specific vehicle ECUs (application-specific); non-CAN protocols on this variant.
  - dependencies: ACX board; CAN transceiver hardware; esp32-core-lib.
- **Requirements**
  - architecture: ACX ESP32 + Riot CAN driver nodes.
  - protocols: CAN bus; Riot Runtime field protocols.
  - power: USB-C / 12VDC.
- **Hardware**
  - hardware_features: ACX I/O set + CAN transceiver.
- **Commercial**
  - target_market: OEMs with CAN-bus equipment (industrial, automotive-adjacent).
  - revenue_model: Hardware sale (ACX) + firmware variant.

---

## 5. ACX-IOB

**dev-pulse:** `e725346c-6411-4411-9f4b-1ebf595140f8` · repo *(none linked)* · tags `product:acx` `docs:riot-runtime` `category:hardware`
**Source:** [ACX Overview](https://nubeio.github.io/rbx-docs/products/acx/overview/) (IO board expansion for ACX)

- **Summary**
  - product_name: **ACX-IOB** (ACX IO Board)
  - objective: Expand the ACX controller's I/O capacity with an add-on IO board.
  - problem: The base ACX has only a few I/O points; some installs need more without stepping up to a full ACB-M.
  - value: Modular I/O growth on the compact ACX platform.
  - differentiators: Stackable with ACX; same Riot Runtime integration.
  - success_criteria: Expanded I/O points all readable/writable via Riot/Rubix. *(No repo linked — confirm hardware scope.)*
- **Scope**
  - in_scope: IO board hardware + Riot Runtime point mapping.
  - out_of_scope: Standalone operation (requires ACX host controller).
  - dependencies: ACX controller; *(repo not yet linked).*
- **Requirements**
  - architecture: Expansion board on ACX bus.
  - protocols: Per Riot Runtime.
  - power: From ACX host.
- **Hardware**
  - hardware_features: Additional DI/RO/UI points beyond base ACX. *(exact count not yet documented — link repo to confirm)*
- **Commercial**
  - target_market: Installations needing mid-range I/O on the ACX platform.
  - revenue_model: Hardware sale (expansion board).

---

## 6. Gen-02 Dashboard (Cloud)

**dev-pulse:** `e9bdaebb-bfbd-4fef-9ea2-3a0fd4968f64` · repos `NubeIO/gen2-cloud`, `NubeIO/rubix` · tags `docs:rubix-frontend` `category:software`
**Source:** [Rubix Frontend › Dashboard](https://nubeio.github.io/rbx-docs/rubix/frontend/dashboard/overview/) + [Rubix Overview](https://nubeio.github.io/rbx-docs/rubix/overview/)

- **Summary**
  - product_name: **Gen-02 Dashboard (Cloud)**
  - objective: Cloud-hosted Rubix Dashboard — at-a-glance node health plus a live widget grid, delivered as a SaaS rather than on-prem.
  - problem: Operating the dashboard required a local gateway/ACB-L; cloud tenants want browser access with no on-site host.
  - value: Same React dashboard codebase running in the cloud, streaming live series over SSE, workspace-scoped and capability-gated.
  - differentiators: Tauri + browser share one codebase; react-grid-layout with persisted per-workspace layouts; node-health cards across all subsystems.
  - success_criteria: Multi-tenant cloud dashboard serving live series + health with workspace isolation.
- **Scope**
  - in_scope: React SPA dashboard; widget grid (time-series, stat, gauge); node-health cards; SSE live streaming; SurrealDB-persisted layouts.
  - out_of_scope: The node backend binary itself (see Gen-02 Software); native Tauri packaging (cloud is browser-only).
  - dependencies: Rubix backend node; SurrealDB; Zenoh bus.
- **Requirements**
  - architecture: React SPA → cloud Rubix gateway → host (capability-gated) → SurrealDB/Zenoh.
  - protocols: HTTPS + SSE for live motion; `series.read`/`series.latest`/`series.find` capability verbs.
- **Hardware**
  - *(software project — n/a)*
- **Commercial**
  - target_market: Facility managers / operators wanting cloud SaaS access to building data.
  - revenue_model: SaaS subscription (cloud-hosted dashboard).

---

## 7. Gen-02 Software

**dev-pulse:** `93bfd630-3e16-426b-8880-6676cef12607` · repo `NubeIO/gen2-software` · tags `docs:rubix-backend` `category:software`
**Source:** [Rubix › Backend Overview](https://nubeio.github.io/rbx-docs/rubix/backend/overview/)

- **Summary**
  - product_name: **Gen-02 Software** (Rubix backend node)
  - objective: The single Rust binary every Rubix node runs — one binary for cloud workstation or local appliance, role selected at boot.
  - problem: Maintaining separate server vs edge codebases doubles effort and causes drift.
  - value: Symmetric nodes — the same binary, same capability model, same datastore — deployed anywhere.
  - differentiators: One datastore (SurrealDB); state vs motion split (SurrealDB vs Zenoh); capability-first with no internal bypass.
  - success_criteria: Boot-time role selection validated across cloud and appliance deployments.
- **Scope**
  - in_scope: Store (SurrealDB), Bus (Zenoh), Host (channels, workspaces, jobs, rules, MCP routing), Gateway (HTTP/SSE edge).
  - out_of_scope: Frontend SPA (see Gen-02 Dashboard); ESP32 firmware (Riot Runtime).
  - dependencies: SurrealDB; Zenoh; Rust toolchain.
- **Requirements**
  - architecture: Single binary; Store + Bus + Host + Gateway layers; workspace isolation by DB namespace.
  - protocols: HTTP/SSE (gateway); Zenoh pub/sub (bus); external DBs federated via `federation` extension.
- **Hardware**
  - *(software project — n/a)*
- **Commercial**
  - target_market: Platform layer underpinning all Rubix deployments (cloud + edge).
  - revenue_model: Enabling software for the Rubix platform / hardware sales.

---

## 8. Holi GW

**dev-pulse:** `4ad29962-2e39-409e-8386-a499adc6bb79` · repo `NubeIO/holi-gw` · tags `product:acbm-home` `product:acbm` `docs:riot-runtime` `category:hardware`
**Source:** [ACB-M Home Gateway Overview](https://nubeio.github.io/rbx-docs/products/gateways/acbm-home-gateway/overview/)

- **Summary**
  - product_name: **Holi GW** (ACB-M Home Gateway)
  - objective: The home/consumer variant of the ACB-M gateway — same ESP32 Riot Runtime, packaged for residential use.
  - problem: The commercial ACB-M is over-specified for home automation; consumers need a simpler, cheaper gateway.
  - value: Full Riot Runtime + Riot packages + protocols at a home price point.
  - differentiators: Home-variant hardware of the proven ACB-M; identical firmware compatibility.
  - success_criteria: Riot packages and protocols working identically to ACB-M on the home board.
- **Scope**
  - in_scope: ESP32 Riot Runtime; same Riot packages and protocols as ACB-M.
  - out_of_scope: CM4/Linux stack (that's ACB-L); commercial-grade I/O density.
  - dependencies: esp32-core-lib; rp-core Riot packages.
- **Requirements**
  - architecture: ESP32 single-board; Riot Runtime firmware.
  - protocols: ACB-M protocol set (Modbus, BACnet, LoRaRAW).
  - power: *(home gateway PSU — confirm spec)*
- **Hardware**
  - hardware_features: ACB-M-equivalent ESP32 platform in a home enclosure.
  - physical_notes: *(Home/consumer enclosure — docs page is a stub; confirm details)*
- **Commercial**
  - target_market: Residential / home automation.
  - revenue_model: Hardware sale (consumer gateway).

---

## 9. Holi solution

**dev-pulse:** `b5c4ea0e-78bf-4300-bd3c-477ca7cb2a9f` · repo `NubeIO/holi-solution` · tags `product:acbm-home` `docs:rubix-frontend` `category:software`
**Source:** [ACB-M Home Gateway](https://nubeio.github.io/rbx-docs/products/gateways/acbm-home-gateway/overview/) + [Rubix Frontend](https://nubeio.github.io/rbx-docs/rubix/frontend/overview/)

- **Summary**
  - product_name: **Holi solution**
  - objective: The end-to-end consumer solution layer over the Holi GW — cloud app + onboarding so a home user gets a working system, not just a box.
  - problem: A gateway alone isn't a product for consumers; they need provisioning, a mobile/web app, and cloud access bundled.
  - value: Turnkey residential offering combining the Holi GW hardware with a cloud Rubix frontend.
  - differentiators: Consumer-grade UX over the industrial Rubix capability model; scan-to-onboard flow.
  - success_criteria: A non-technical user can commission and operate the system unaided.
- **Scope**
  - in_scope: Cloud frontend/app; device onboarding; consumer-facing UX over Rubix capabilities.
  - out_of_scope: The gateway firmware (see Holi GW); commercial BMS features.
  - dependencies: Holi GW hardware; Rubix backend; Rubix frontend.
- **Requirements**
  - architecture: Consumer app → cloud Rubix → Holi GW (Riot Runtime).
  - protocols: HTTPS/SSE; scan-based commissioning (Easy Add pattern).
- **Hardware**
  - *(software/solution — n/a)*
- **Commercial**
  - target_market: Residential consumers / prosumer home automation.
  - revenue_model: Bundled hardware + cloud subscription.

---

## 10. IO-22-BAC-PRG

**dev-pulse:** `e154ba28-b80d-41dd-9b4a-30523b6cdd1c` · repo `NubeIO/io22` · tags `product:io-22` `docs:riot-runtime` `category:firmware`
**Source:** [IO-22 (UIUO)](https://nubeio.github.io/rbx-docs/products/io-controllers/io-22-uiuo/overview/) + [IO-22 (14DI, 8RO)](https://nubeio.github.io/rbx-docs/products/io-controllers/io-22-14di-8ro/overview/)

- **Summary**
  - product_name: **IO-22 Programmable (BACnet)** — IO-8UIUO-4DI-4AOV-6RO + IO-14DI-8RO
  - objective: Deliver the IO-22 expansion cards in their **programmable** config (ACB-M bottom PCB), running the Riot Runtime for local logic.
  - problem: Dumb IO cards can't do local control logic; many sites need edge decisions, not just remote points.
  - value: Full Riot Runtime processing + logic on the IO-22, standalone or as part of a larger system.
  - differentiators: Per-channel software-configurable universal I/O (AD74412R, no jumpers); 16-bit ADC/DAC; BACnet configuration.
  - success_criteria: Both IO-22 variants programmable via Riot Runtime with BACnet config.
- **Scope**
  - in_scope: IO-22 UIUO (8 UI + 4 DI + 4 AOV + 6 RO) and IO-22 14DI/8RO, ACB-M-based programmable mode.
  - out_of_scope: Dumb-IO mode (that's the ACBP variant); LoRaWAN (use ACBL).
  - dependencies: ACB-M bottom PCB; Riot Runtime; esp32-core-lib.
- **Requirements**
  - architecture: ACB-M PCB running Riot Runtime; ACBM-IO-22 expansion bus.
  - protocols: BACnet (configuration); Riot field protocols.
  - power: 24V/DC rail.
- **Hardware**
  - hardware_features: 8× UIUO (V/I in-out, DI, RTD, 16-bit), 4× DI, 4× AOV, 6× RO + 14× DI / 8× RO variant; AD74412R engine; 500V per-channel isolation; TUE ±0.1% FSR.
  - physical_notes: Plug-in expansion card or ACBM stack-on module.
- **Commercial**
  - target_market: Building automation, HVAC, facilities needing edge-programmable IO.
  - revenue_model: Hardware sale (IO cards + ACB-M).

---

## 11. IO-22-BAC+CLOUD-PRG

**dev-pulse:** `fadf7643-38c6-420c-bbbc-e79c5f82d6a1` · repo *(none linked)* · tags `product:io-22` `docs:riot-runtime` `category:firmware`
**Source:** [IO-22 (UIUO)](https://nubeio.github.io/rbx-docs/products/io-controllers/io-22-uiuo/overview/) + [Easy Add](https://nubeio.github.io/rbx-docs/easy-add/overview/)

- **Summary**
  - product_name: **IO-22 Programmable (BACnet + Cloud provisioning)**
  - objective: Same programmable IO-22 hardware, plus a cloud-based provisioning/commissioning path (scan-to-add).
  - problem: On-site BACnet-only commissioning is slow; cloud provisioning lets installers add devices remotely.
  - value: Combines Riot Runtime programmability with Easy Add scan-based cloud onboarding.
  - differentiators: BACnet config + cloud scan commissioning on the same card.
  - success_criteria: Device commissionable via cloud scan AND locally programmable via BACnet.
- **Scope**
  - in_scope: IO-22 programmable hardware; Easy Add cloud commissioning flow.
  - out_of_scope: Pure dumb-IO mode; LoRaWAN.
  - dependencies: ACB-M PCB; Riot Runtime; Rubix cloud node + Easy Add. *(repo not yet linked)*
- **Requirements**
  - architecture: ACB-M IO-22 + cloud Rubix provisioning.
  - protocols: BACnet (local config); HTTPS/SSE (cloud provisioning).
  - power: 24V/DC rail.
- **Hardware**
  - hardware_features: IO-22 UIUO + 14DI/8RO I/O set (see entry 10).
- **Commercial**
  - target_market: Installers wanting remote/cloud device onboarding.
  - revenue_model: Hardware sale + cloud onboarding service.

---

## 12. IO-22-EXPANSION

**dev-pulse:** `83b083fb-927d-459e-8a77-2e757c4452c4` · repo *(none linked)* · tags `product:io-22` `docs:riot-runtime` `category:hardware`
**Source:** [IO-22 (UIUO)](https://nubeio.github.io/rbx-docs/products/io-controllers/io-22-uiuo/overview/) (expansion mode)

- **Summary**
  - product_name: **IO-22 Expansion**
  - objective: Deliver the IO-22 cards in **expansion mode** — connecting to ACBM controllers over the ACBM-IO-22 expansion bus.
  - problem: ACBM-based systems need more I/O points than the controller carries natively.
  - value: Standard expansion mode with full Riot Runtime integration on the main controller.
  - differentiators: Hot-add up to 4 modules per ACBL side; seamless Riot point mapping.
  - success_criteria: Expanded I/O visible and controllable through the host ACBM controller.
- **Scope**
  - in_scope: IO-22 UIUO + 14DI/8RO in ACBM expansion-bus mode.
  - out_of_scope: Standalone/programmable mode (entry 10); dumb-IO/ACBP mode.
  - dependencies: ACBM controller; ACBM-IO-22 expansion bus. *(repo not yet linked)*
- **Requirements**
  - architecture: Expansion cards on ACBM-IO-22 bus, driven by host Riot Runtime.
  - protocols: Expansion-bus; BACnet via host.
  - power: 24V/DC from host rail.
- **Hardware**
  - hardware_features: IO-22 UIUO + 14DI/8RO I/O set; max 4 modules right-side on ACBL.
  - physical_notes: Plug-in / stack-on expansion module.
- **Commercial**
  - target_market: Existing ACBM/ACBL installs needing more points.
  - revenue_model: Hardware sale (expansion cards).

---

## 13. Insurance Project (PoC)

**dev-pulse:** `e924a9eb-4c30-48d5-81ca-2c3ca75f6e53` · repo `NubeIO/iag-solution` · tags `product:lora` `docs:easy-add` `category:software` `PoC`
**Source:** [LoRa Overview](https://nubeio.github.io/rbx-docs/products/lora/overview/) + [Micro Edge](https://nubeio.github.io/rbx-docs/products/lora/micro-edge/overview/) + [Easy Add](https://nubeio.github.io/rbx-docs/easy-add/overview/)

- **Summary**
  - product_name: **Insurance Project (PoC)** — IAG water-leak mitigation solution
  - objective: Prove an end-to-end water-damage mitigation bundle: a cloud app + water-flow monitoring + leak sensing + an optional LoRa water valve.
  - problem: Insurers need verifiable, low-cost leak detection + auto-shutoff to reduce water-damage claims.
  - value: LoRa sensor mesh (flow, leak, optional valve) feeding a cloud app, commissioned via scan, at PoC cost.
  - differentiators: LoRaRAW (no LoRaWAN infra needed); ACB-M micro gateway; scan-based onboarding.
  - success_criteria: Leak → detection → cloud alert → optional valve close, demonstrated end to end.
- **Scope**
  - in_scope: (1) Insurance app, (2) water-flow monitoring (Micro Edge pulse input), (3) leak sensor, (4) optional LoRa water valve.
  - out_of_scope: LoRaWAN infrastructure; non-water perils; full claims integration.
  - dependencies: ACB-M (LoRaRAW gateway); Micro Edge sensors; LoRa water valve; Rubix cloud app.
- **Requirements**
  - architecture: LoRaRAW sensors → ACB-M micro gateway → Rubix cloud app.
  - protocols: LoRaRAW; HTTPS/SSE to cloud.
  - power: Battery (sensors); mains (gateway/valve).
- **Hardware**
  - hardware_features: Micro Edge (water pulse + optical + thermistor); LoRa leak sensor; LoRa water valve; ACB-M gateway.
- **Commercial**
  - target_market: Property insurers, strata/facility risk managers.
  - revenue_model: PoC toward a per-premise subscription / hardware bundle.

---

## 14. ME-02

**dev-pulse:** `4fc5ab50-f19a-4460-8766-15dd870d74e9` · repo `NubeIO/me-optical` · tags `product:micro-edge` `product:lora` `docs:riot-runtime` `category:firmware`
**Source:** [LoRa › Micro Edge](https://nubeio.github.io/rbx-docs/products/lora/micro-edge/overview/)

- **Summary**
  - product_name: **ME-02** (Micro Edge) — "Optical, Pulse, Leak"
  - objective: Battery-powered LoRa end-device for metering/sensing — water pulse, optical meter pickup, and temperature.
  - problem: Reading utility meters and leak conditions wirelessly needs a low-power edge node with multiple input types.
  - value: One battery LoRa device covering pulse (water meter), optical (electricity meter LED), and a 10k thermistor.
  - differentiators: LoRaRAW direct-to-ACB-M gateway (no LoRaWAN); multi-input in one node; battery-powered.
  - success_criteria: All three input types reporting reliably over LoRaRAW to the gateway.
- **Scope**
  - in_scope: Water pulse input; optical (LED) pickup input; 10k thermistor input; LoRaRAW radio; battery power.
  - out_of_scope: LoRaWAN; mains power; on-board control logic (sensor-only).
  - dependencies: ACB-M acting as LoRaRAW micro gateway; `rp_lorarawgw` Riot package.
- **Requirements**
  - architecture: Battery LoRa end-device → ACB-M LoRaRAW gateway → Rubix.
  - protocols: LoRaRAW.
  - power: Battery.
- **Hardware**
  - hardware_features: 3 input types (pulse, optical, thermistor); LoRa radio; battery.
- **Commercial**
  - target_market: Sub-metering, water/energy metering, leak monitoring.
  - revenue_model: Hardware sale (sensor node).

---

## 15. riot

**dev-pulse:** `0c0ab916-6961-4590-ba31-3ab4c54b0c61` · repo `NubeIO/rp-core` · tags `docs:riot` `category:software`
**Source:** [Riot Overview](https://nubeio.github.io/rbx-docs/riot/overview/) + [Riot Runtime](https://nubeio.github.io/rbx-docs/riot-runtime/overview/)

- **Summary**
  - product_name: **Riot** (rp-core dataflow execution engine)
  - objective: A node-graph execution engine where each node performs a discrete function and passes values along edges — functionality delivered as runtime-loaded packages.
  - problem: Hard-coding control logic per device is unmaintainable; the platform needs a portable, package-based logic layer.
  - value: One engine runs on Linux (generic), ACB-M (ESP32), and IO-16, sharing node packages.
  - differentiators: Packages as `.so` shared libraries loaded at runtime; core packs (io, math, logic, hvac/PID) + protocol packs (modbus, bacnet, lorarawgw); GoogleTest per package.
  - success_criteria: Engine evaluates node graphs identically across all device targets.
- **Scope**
  - in_scope: Engine + node package format; rp_io, rp_math, rp_logic, rp_compare, rp_latch, rp_hvac, rp_statistics, rp_time, rp_vp; protocol packages (modbus/bacnet master-slave, lorarawgw, dev_settings).
  - out_of_scope: The ESP32 hardware-abstraction layer (that's Riot Runtime / esp32-core-lib); the Rubix host/backend.
  - dependencies: rp-core repo (dev branch); riot-rust engine; CMake build; Riot Runtime on ESP32 targets.
- **Requirements**
  - architecture: Graph evaluator loading `.so` packages; compile-define targets `RIOT_DEV_GENERIC` / `_ACBM` / `_IO16`.
  - protocols: Driven by loaded packages (Modbus, BACnet, LoRaRAW).
- **Hardware**
  - *(software engine — runs on Linux + ESP32)*
- **Commercial**
  - target_market: Platform enabler — every NubeIO edge device runs Riot graphs.
  - revenue_model: Enabling software underpinning hardware sales.

---

## 16. Scan to Dashboard

**dev-pulse:** `41c7f814-15f7-4f66-a2dd-13162b790127` · repo `NubeIO/scan-to-dashboard` · tags `docs:easy-add` `category:software`
**Source:** [Easy Add Overview](https://nubeio.github.io/rbx-docs/easy-add/overview/) + [Rubix Dashboard](https://nubeio.github.io/rbx-docs/rubix/frontend/dashboard/overview/)

- **Summary**
  - product_name: **Scan to Dashboard**
  - objective: Scan-based device commissioning — scan a QR/NFC tag to discover and provision a device straight into the Rubix Dashboard, no manual IP or config files.
  - problem: Manual IP entry and config-file commissioning is error-prone and slow for installers.
  - value: Scan → discover → provision, getting a device live in the dashboard in seconds.
  - differentiators: QR/NFC scan discovery; covers Rubix devices (LoRa, ACBx, IO-22) and Riot Runtime 2-way LoRa commissioning.
  - success_criteria: An installer commissions a device to the dashboard via scan with zero manual network config.
- **Scope**
  - in_scope: User sign-up flow; adding Rubix devices (LoRa sensors, ACBx, IO-22); adding Riot Runtime devices with 2-way LoRa commissioning.
  - out_of_scope: Device firmware itself; the dashboard widget grid (separate concern).
  - dependencies: Rubix Dashboard + backend; devices with QR/NFC tags.
- **Requirements**
  - architecture: Dashboard commissioning flow → Rubix backend → device provisioning.
  - protocols: QR/NFC scan discovery; HTTPS; LoRa 2-way commissioning for Riot Runtime devices.
- **Hardware**
  - *(software flow — n/a)*
- **Commercial**
  - target_market: Installers/commissioners of Rubix + Riot Runtime devices.
  - revenue_model: Enabling UX for the platform (reduces install cost).

---

## 17. TMVM v2.0 (OEM)

**dev-pulse:** `99a133c0-16e6-44e5-a8e2-cd39698d20c0` · repos `NubeIO/ACX-hardware`, `NubeIO/galvin-tmv` · tags `product:tmv` `docs:product` `category:hardware`
**Source:** [OEM › TMV Overview](https://nubeio.github.io/rbx-docs/products/oem/tmv/overview/)

- **Summary**
  - product_name: **TMVM v2.0 (OEM)** — CliniMix-TMV connected monitoring transmitter (Gen 2)
  - objective: Transform Galvin Engineering's CliniMix Thermostatic Mixing Valve into a connected, auditable asset for proactive water-safety compliance via direct-to-cloud architecture.
  - problem: Traditional TMVs are unmonitored; healthcare/commercial facilities can't prove compliance or catch scald/legionella risk proactively.
  - value: Continuous visibility, per-event compliance summaries, and remote valve control — no on-site controller needed (single Ethernet drop).
  - differentiators: Direct device-to-cloud (no gateway); built on ACX/ESP32-S3; flow-gated NTC measurement (<1% tolerance); PoE powers device + valve; IPx5; RCM marked; references AS 4032.3 maintenance logic.
  - success_criteria: MIN/MAX/AVG/COMP temp + scald flags + flow events pushed to cloud; solenoid control validated.
- **Scope**
  - in_scope: Outlet temp (NTC, flow-gated), flow-switch sensing, solenoid valve drive, on-device per-event computation, Wi-Fi AP commissioning, cloud push.
  - out_of_scope: The mechanical TMV itself (Galvin's); on-site BMS supervisory control.
  - dependencies: ACX platform (ESP32-S3-WROOM-1U); Riot Runtime; Galvin CliniMix-TMV; Wallgate WVPB400-2 reference solenoid; cloud Rubix.
- **Requirements**
  - architecture: Direct device-to-cloud node — TMV → ACX transmitter → LAN → Rubix cloud (no on-site controller).
  - protocols: Ethernet/PoE to cloud; Wi-Fi AP for commissioning; RJ12 expansion to remote I/O over BACnet/Modbus.
  - power: PoE (primary, powers device + valve); USB-C (commissioning/USB PD 3.2); 12VDC field alternative. USB + PoE concurrent-safe.
  - mounting: TMV cabinet; solenoid via male 3-pin mini-XLR; probe via 2-pin IP65 plug.
- **Hardware**
  - hardware_features: ESP32-S3-WROOM-1U (16 MB flash + 2 MB PSRAM, WiFi+BLE); onboard RTC; secure OTA; immutable GUID identity; solenoid driver (6–24V DC, current-limiting/flyback/protected); FIFO buffer + optional SD history.
  - physical_notes: 0–65 °C operating; IPx5; EN IEC 61000-6-3:2021, AS/NZS 62368.1 / CISPR 32 / 4268; RCM marked.
- **Commercial**
  - target_market: Healthcare & commercial facilities water-safety compliance (CliniMix/Galvin OEM channel).
  - revenue_model: OEM hardware (transmitter) + cloud compliance/monitoring.

---

## 18. UART

**dev-pulse:** `8a280f35-9b70-4793-922f-65cb4b8b6aef` · repo `NubeIO/fga-uart-fw` · tags `product:fga-uart` `docs:riot-runtime` `category:firmware`
**Source:** [OEM › FGA UART Overview](https://nubeio.github.io/rbx-docs/products/oem/fga-uart/overview/)

- **Summary**
  - product_name: **FGA UART** (Fujitsu General AC UART bridge)
  - objective: A LoRa device that connects to Fujitsu AC units over their native UART protocol and bridges them into Rubix.
  - problem: Fujitsu AC units use a proprietary UART protocol not reachable by standard BACnet/Modbus controllers.
  - value: Native Fujitsu UART integration + LoRa wireless backhaul — no wired bus to the controller.
  - differentiators: Fujitsu UART protocol support; LoRaRAW transport; powered by the AC unit itself (no separate PSU).
  - success_criteria: Bidirectional command/status with reference Fujitsu AC units over LoRa.
- **Scope**
  - in_scope: Fujitsu AC UART protocol bridge; LoRaRAW radio.
  - out_of_scope: Non-Fujitsu AC brands; wired (non-LoRa) connection.
  - dependencies: ACB-M LoRaRAW gateway; Fujitsu AC unit (power source + target).
- **Requirements**
  - architecture: AC unit → FGA UART bridge (LoRa) → ACB-M gateway → Rubix.
  - protocols: Fujitsu UART; LoRaRAW.
  - power: Supplied by the AC unit (not battery, not external).
- **Hardware**
  - hardware_features: LoRa radio + Fujitsu UART interface; draws power from the host AC unit.
- **Commercial**
  - target_market: HVAC integrators deploying Fujitsu General ACs.
  - revenue_model: Hardware sale (bridge device).

---

## 19. ZC Daikin

**dev-pulse:** `7924445c-38ae-4e4e-84b0-513a9cb46ccb` · repo `NubeIO/zc-daikin` · tags `product:zone-controller` `docs:riot-runtime` `category:firmware`
**Source:** [Zone Controller › Daikin P1P2](https://nubeio.github.io/rbx-docs/products/oem/zone-controller/daikin-p1p2/overview/) + [Zone Controller](https://nubeio.github.io/rbx-docs/products/oem/zone-controller/overview/)

- **Summary**
  - product_name: **ZC Daikin** (Zone Controller — Daikin P1P2 variant)
  - objective: Zone-control firmware variant speaking Daikin's P1P2 protocol, built on the ACB-M hardware platform.
  - problem: Daikin ACs use the P1P2 bus; standard controllers can't address Daikin zones natively.
  - value: Native Daikin P1P2 zone control from the ACB-M-based zone controller, with IO-ZC damper control.
  - differentiators: ACB-M hardware + custom zone-control firmware (not Riot Runtime); IO-ZC 10× relay damper drive; Droplet LoRa temp/humidity sensor accessory.
  - success_criteria: Daikin P1P2 communication + zone damper control validated. *(Daikin P1P2 docs page is currently a stub — details to confirm.)*
- **Scope**
  - in_scope: ACB-M-based zone controller; Daikin P1P2 protocol; IO-ZC damper relay expansion; Droplet sensor.
  - out_of_scope: Riot node-graph execution (zone controller uses custom firmware, not Riot).
  - dependencies: ACB-M PCB + esp32-core-lib; IO-ZC accessory; Daikin reference unit.
- **Requirements**
  - architecture: ACB-M ESP32 hardware; custom zone-control application firmware.
  - protocols: Daikin P1P2; *(other ACB-M field protocols as applicable).*
  - power: *(ACB-M standard — confirm)*
- **Hardware**
  - hardware_features: ACB-M GPIO/I2C/UART/IO-expanders/Ethernet/LoRa/RTC/FRAM; IO-ZC 10× RO over RJ12.
- **Commercial**
  - target_market: Commercial HVAC zoned systems using Daikin equipment.
  - revenue_model: Hardware sale (zone controller + IO-ZC accessories).

---

## 20. Zoneconnex V2

**dev-pulse:** `fc06abbe-bc5f-4d71-ae50-2913edcc1052` · repo `NubeIO/zoneconnex-2` · tags `product:zone-controller` `docs:riot-runtime` `category:hardware`
**Source:** [Zone Controller Overview](https://nubeio.github.io/rbx-docs/products/oem/zone-controller/overview/)

- **Summary**
  - product_name: **Zoneconnex V2** (Zone Controller, generation 2)
  - objective: The second-generation zone-control device — ACB-M hardware platform running purpose-built zone-control firmware.
  - problem: Zone control needs dedicated damper/equipment logic that general-purpose Riot graphs don't cleanly provide.
  - value: A focused zone-control product with its own firmware, plus IO-ZC damper and Droplet sensor accessories.
  - differentiators: Custom zone firmware (not Riot); IO-ZC 10× RO damper drive via RJ12; Droplet LoRa temp/humidity sensor; shares esp32-core-lib with ACB-M.
  - success_criteria: Multi-zone damper control + sensing delivered as a turnkey product.
- **Scope**
  - in_scope: ACB-M-based zone controller + custom firmware; IO-ZC relay expansion; Droplet sensor integration.
  - out_of_scope: Riot node-graph execution; general BMS supervisory control.
  - dependencies: ACB-M PCB; esp32-core-lib; IO-ZC + Droplet accessories.
- **Requirements**
  - architecture: ACB-M ESP32; custom zone-control application layer.
  - protocols: ACB-M field protocols; LoRa (to Droplet).
  - power: *(ACB-M standard — confirm)*
- **Hardware**
  - hardware_features: ACB-M hardware set (GPIO, I2C, UART, IO expanders, Ethernet, LoRa, RTC, FRAM); IO-ZC 10× RO.
- **Commercial**
  - target_market: Commercial / residential zoned HVAC.
  - revenue_model: Hardware sale (controller + accessories).

---

## Notes on sourcing & gaps

- **Stub docs pages** (thin content): ACBM Home Gateway, Daikin P1P2 — fields marked *(confirm)* should be verified once those pages are fleshed out.
- **No repo linked:** ACX-IOB, IO-22-BAC+CLOUD-PRG, IO-22-EXPANSION — hardware scope inferred from name/description; link repos to confirm.
- **"Holi" naming** is internal jargon for the ACB-M Home Gateway line; the mapping is confident but not literally stated in docs.
- When ready to load these into the app, each entry maps 1:1 to the
  `PATCH /projects/{id}/exec-summary` section objects — see
  [READING-PROJECTS.md §7](READING-PROJECTS.md).
