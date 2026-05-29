import type { ExecSummaryDto } from "../../../api/client.js";
import { useExecSummaryAutosave } from "../hooks/use-exec-summary.js";
import {
  DateField,
  MarkdownField,
  TextField,
} from "../form-fields.js";

export function SummarySection({
  projectId,
  data,
}: {
  projectId: string;
  data: ExecSummaryDto;
}): JSX.Element {
  const { patch } = useExecSummaryAutosave(projectId);
  const s = data.summary;
  return (
    <div className="flex flex-col gap-6">
      <div className="grid grid-cols-1 gap-4 md:grid-cols-3">
        <div data-validation-key="summary.product_name">
          <TextField
            id="es-product-name"
            label="Product name"
            value={s.product_name}
            onCommit={(product_name) => patch({ summary: { product_name } })}
            placeholder="e.g. Edge Controller 8X"
          />
        </div>
        <TextField
          id="es-part-number"
          label="Part number"
          value={s.part_number}
          onCommit={(part_number) => patch({ summary: { part_number } })}
          placeholder="EC-8X-R2"
        />
        <DateField
          id="es-target-release"
          label="Target release date"
          value={s.target_release_date}
          onCommit={(target_release_date) =>
            patch({ summary: { target_release_date } })
          }
        />
      </div>
      <div data-validation-key="summary.objective">
        <MarkdownField
          label="Objective"
          value={s.objective}
          onCommit={(objective) => patch({ summary: { objective } })}
          hint="What this product is and the headline goal it serves."
        />
      </div>
      <MarkdownField
        label="Problem"
        value={s.problem}
        onCommit={(problem) => patch({ summary: { problem } })}
        hint="The customer pain or market gap this product addresses."
      />
      <MarkdownField
        label="Value"
        value={s.value}
        onCommit={(value) => patch({ summary: { value } })}
      />
      <MarkdownField
        label="Differentiators"
        value={s.differentiators}
        onCommit={(differentiators) => patch({ summary: { differentiators } })}
      />
      <div data-validation-key="summary.success_criteria">
        <MarkdownField
          label="Success criteria"
          value={s.success_criteria}
          onCommit={(success_criteria) =>
            patch({ summary: { success_criteria } })
          }
          hint="Measurable outcomes — pricing, shipped units, attach rate, NPS, etc."
        />
      </div>
    </div>
  );
}
