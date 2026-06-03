/**
 * View-wizard barrel — single import surface for the create
 * wizard, the edit dialog, and the helpers the workbench / add
 * dialog need to wire them up (slug normalisation, tag lookup,
 * auto-tag on issue create).
 */

export { NewViewWizard } from "./wizard-dialog.js";
export { EditViewDialog } from "./edit-dialog.js";
export type { EditViewDialogProps } from "./edit-dialog.js";
export {
  CATEGORY_TAG_KEY,
  categoryTagName,
  ensureCategoryTag,
  ensureCategoryTags,
  findCategoryTag,
  slugifyCategoryKey,
} from "./category-utils.js";
export {
  CATEGORISED_GROUP_BY,
  CATEGORY_CHIPS,
  CATEGORY_PACKS,
  type CategoryChip,
  type CategoryPack,
} from "./templates.js";
export { DateDisplayPicker } from "./date-display-picker.js";
export type { DateDisplayPickerProps } from "./date-display-picker.js";
export {
  formatAu,
  formatDateDisplay,
  readCompleted,
  readDateDisplayMode,
  weekOfMonthLabel,
  writeCompleted,
  writeDateDisplayMode,
  type DateDisplayMode,
} from "./date-display.js";
