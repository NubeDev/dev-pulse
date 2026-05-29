import type { ExecSummaryDto } from "../../../api/client.js";
import { PROTOCOL_OPTIONS } from "../../../api/client.js";
import { useExecSummaryAutosave } from "../hooks/use-exec-summary.js";
import { CheckboxGroup, MarkdownField, TextField } from "../form-fields.js";

export function RequirementsSection({
  projectId,
  data,
}: {
  projectId: string;
  data: ExecSummaryDto;
}): JSX.Element {
  const { patch } = useExecSummaryAutosave(projectId);
  const r = data.requirements;
  return (
    <div className="flex flex-col gap-6">
      <div data-validation-key="requirements.must_have">
        <MarkdownField
          label="Must have"
          value={r.must_have}
          onCommit={(must_have) => patch({ requirements: { must_have } })}
        />
      </div>
      <MarkdownField
        label="Optional"
        value={r.optional}
        onCommit={(optional) => patch({ requirements: { optional } })}
      />
      <MarkdownField
        label="User interaction"
        value={r.user_interaction}
        onCommit={(user_interaction) =>
          patch({ requirements: { user_interaction } })
        }
      />
      <MarkdownField
        label="Architecture"
        value={r.architecture}
        onCommit={(architecture) => patch({ requirements: { architecture } })}
      />
      <div data-validation-key="requirements.protocols">
        <CheckboxGroup
          label="Protocols"
          options={PROTOCOL_OPTIONS}
          value={r.protocols ?? []}
          onCommit={(protocols) => patch({ requirements: { protocols } })}
          hint="Field protocols the product must support."
        />
      </div>
      <div className="grid grid-cols-1 gap-4 md:grid-cols-3">
        <TextField
          id="es-power"
          label="Power"
          value={r.power}
          onCommit={(power) => patch({ requirements: { power } })}
          placeholder="24 VDC / PoE / mains"
        />
        <TextField
          id="es-mounting"
          label="Mounting"
          value={r.mounting}
          onCommit={(mounting) => patch({ requirements: { mounting } })}
          placeholder="DIN rail, wall, panel"
        />
        <TextField
          id="es-certification"
          label="Certification"
          value={r.certification}
          onCommit={(certification) =>
            patch({ requirements: { certification } })
          }
          placeholder="CE, FCC, UL"
        />
      </div>
    </div>
  );
}
