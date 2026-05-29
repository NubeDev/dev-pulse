import type { ExecSummaryDto } from "../../../api/client.js";
import { useExecSummaryAutosave } from "../hooks/use-exec-summary.js";
import { MarkdownField } from "../form-fields.js";

export function ScopeSection({
  projectId,
  data,
}: {
  projectId: string;
  data: ExecSummaryDto;
}): JSX.Element {
  const { patch } = useExecSummaryAutosave(projectId);
  const s = data.scope;
  return (
    <div className="flex flex-col gap-6">
      <div data-validation-key="scope.in_scope">
        <MarkdownField
          label="In scope"
          value={s.in_scope}
          onCommit={(in_scope) => patch({ scope: { in_scope } })}
        />
      </div>
      <div data-validation-key="scope.out_of_scope">
        <MarkdownField
          label="Out of scope"
          value={s.out_of_scope}
          onCommit={(out_of_scope) => patch({ scope: { out_of_scope } })}
        />
      </div>
      <MarkdownField
        label="Assumptions"
        value={s.assumptions}
        onCommit={(assumptions) => patch({ scope: { assumptions } })}
      />
      <MarkdownField
        label="Dependencies"
        value={s.dependencies}
        onCommit={(dependencies) => patch({ scope: { dependencies } })}
      />
      <MarkdownField
        label="Constraints"
        value={s.constraints}
        onCommit={(constraints) => patch({ scope: { constraints } })}
      />
    </div>
  );
}
