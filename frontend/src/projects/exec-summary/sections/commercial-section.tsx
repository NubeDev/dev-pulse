import type { ExecSummaryDto } from "../../../api/client.js";
import { useExecSummaryAutosave } from "../hooks/use-exec-summary.js";
import {
  MarkdownField,
  NumberField,
  TextField,
} from "../form-fields.js";

export function CommercialSection({
  projectId,
  data,
}: {
  projectId: string;
  data: ExecSummaryDto;
}): JSX.Element {
  const { patch } = useExecSummaryAutosave(projectId);
  const c = data.commercial;
  return (
    <div className="flex flex-col gap-6">
      <div className="grid grid-cols-1 gap-4 md:grid-cols-3">
        <NumberField
          id="es-rrp"
          label="RRP"
          value={c.rrp_cents}
          onCommit={(rrp_cents) => patch({ commercial: { rrp_cents } })}
          scale={100}
          step={0.01}
          prefix="$"
          placeholder="0.00"
          hint="Recommended retail price."
        />
        <NumberField
          id="es-oem"
          label="OEM price"
          value={c.oem_price_cents}
          onCommit={(oem_price_cents) =>
            patch({ commercial: { oem_price_cents } })
          }
          scale={100}
          step={0.01}
          prefix="$"
          placeholder="0.00"
        />
        <NumberField
          id="es-gp"
          label="Target GP"
          value={c.target_gp_pct}
          onCommit={(target_gp_pct) =>
            patch({ commercial: { target_gp_pct } })
          }
          step={0.1}
          suffix="%"
          placeholder="0"
        />
      </div>
      <div className="grid grid-cols-1 gap-4 md:grid-cols-2">
        <TextField
          id="es-revenue-model"
          label="Revenue model"
          value={c.revenue_model}
          onCommit={(revenue_model) =>
            patch({ commercial: { revenue_model } })
          }
          placeholder="One-off / subscription / hybrid"
        />
        <TextField
          id="es-channel"
          label="Channel strategy"
          value={c.channel_strategy}
          onCommit={(channel_strategy) =>
            patch({ commercial: { channel_strategy } })
          }
          placeholder="Direct / distributor / OEM"
        />
      </div>
      <MarkdownField
        label="Target market"
        value={c.target_market}
        onCommit={(target_market) =>
          patch({ commercial: { target_market } })
        }
      />
      <MarkdownField
        label="Volume assumptions"
        value={c.volume_assumptions}
        onCommit={(volume_assumptions) =>
          patch({ commercial: { volume_assumptions } })
        }
      />
    </div>
  );
}
