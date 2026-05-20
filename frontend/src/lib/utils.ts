import { clsx, type ClassValue } from "clsx"
import { twMerge } from "tailwind-merge"

/** shadcn `cn` helper — combines clsx + tailwind-merge so utility
 *  classes from props deduplicate against the component's defaults. */
export function cn(...inputs: ClassValue[]) {
  return twMerge(clsx(inputs))
}
