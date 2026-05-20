/**
 * Block-derived `DataTable` — verbatim shadcn dashboard-01
 * `Tabs`-in-toolbar + sticky-headered `Table` shell, with the
 * canned drag-handle / checkbox-select / row-drawer / target-input
 * affordances dropped (none fit dev-pulse data) and the demo
 * columns/schema replaced by a generic `<TData>` prop.
 *
 * The TabsList lives in the table toolbar so each report can hang
 * its three-lens toggle (single_org / all_orgs_combined / per_org_split)
 * off the same surface — the brief calls this out specifically.
 *
 * Pagination is preserved when the row count exceeds `pageSize` —
 * the same `Page X of Y` + prev/next ladder the block ships.
 */

import * as React from "react"
import {
  IconChevronLeft,
  IconChevronRight,
  IconChevronsLeft,
  IconChevronsRight,
} from "@tabler/icons-react"
import {
  flexRender,
  getCoreRowModel,
  getPaginationRowModel,
  getSortedRowModel,
  useReactTable,
  type ColumnDef,
  type SortingState,
} from "@tanstack/react-table"

import { Button } from "@/components/ui/button"
import { Label } from "@/components/ui/label"
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select"
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table"
import {
  Tabs,
  TabsContent,
  TabsList,
  TabsTrigger,
} from "@/components/ui/tabs"

export interface DataTableTab {
  value: string
  label: React.ReactNode
}

export interface DataTableProps<TData> {
  data: TData[]
  columns: ColumnDef<TData, unknown>[]
  /** Optional tabs in the table toolbar (e.g. the three-lens toggle). */
  tabs?: DataTableTab[]
  /** Controlled tab value — required when `tabs` is provided. */
  activeTab?: string
  onTabChange?: (value: string) => void
  /** Optional right-aligned toolbar slot. */
  toolbar?: React.ReactNode
  /** Stable test id on the outer wrapper. */
  testId?: string
  /** Initial page size for client pagination. Defaults to 10. */
  pageSize?: number
  /** Optional row id getter. */
  getRowId?: (row: TData, index: number) => string
  /** Optional placeholder when there are zero rows. */
  emptyMessage?: React.ReactNode
}

export function DataTable<TData>({
  data,
  columns,
  tabs,
  activeTab,
  onTabChange,
  toolbar,
  testId,
  pageSize = 10,
  getRowId,
  emptyMessage = "No results.",
}: DataTableProps<TData>) {
  const [sorting, setSorting] = React.useState<SortingState>([])
  const [pagination, setPagination] = React.useState({
    pageIndex: 0,
    pageSize,
  })

  const table = useReactTable({
    data,
    columns,
    state: { sorting, pagination },
    onSortingChange: setSorting,
    onPaginationChange: setPagination,
    getCoreRowModel: getCoreRowModel(),
    getSortedRowModel: getSortedRowModel(),
    getPaginationRowModel: getPaginationRowModel(),
    ...(getRowId ? { getRowId } : {}),
  })

  const showPagination = data.length > pageSize
  const tabValue = activeTab ?? tabs?.[0]?.value ?? "all"

  const body = (
    <>
      <div className="overflow-hidden rounded-lg border">
        <Table>
          <TableHeader className="sticky top-0 z-10 bg-muted">
            {table.getHeaderGroups().map((headerGroup) => (
              <TableRow key={headerGroup.id}>
                {headerGroup.headers.map((header) => (
                  <TableHead key={header.id} colSpan={header.colSpan}>
                    {header.isPlaceholder
                      ? null
                      : flexRender(
                          header.column.columnDef.header,
                          header.getContext(),
                        )}
                  </TableHead>
                ))}
              </TableRow>
            ))}
          </TableHeader>
          <TableBody>
            {table.getRowModel().rows?.length ? (
              table.getRowModel().rows.map((row) => (
                <TableRow key={row.id}>
                  {row.getVisibleCells().map((cell) => (
                    <TableCell key={cell.id}>
                      {flexRender(
                        cell.column.columnDef.cell,
                        cell.getContext(),
                      )}
                    </TableCell>
                  ))}
                </TableRow>
              ))
            ) : (
              <TableRow>
                <TableCell
                  colSpan={columns.length}
                  className="h-24 text-center text-muted-foreground"
                >
                  {emptyMessage}
                </TableCell>
              </TableRow>
            )}
          </TableBody>
        </Table>
      </div>
      {showPagination ? (
        <div className="flex items-center justify-between px-4">
          <div className="hidden flex-1 text-sm text-muted-foreground lg:flex">
            {table.getFilteredRowModel().rows.length} row(s).
          </div>
          <div className="flex w-full items-center gap-8 lg:w-fit">
            <div className="hidden items-center gap-2 lg:flex">
              <Label htmlFor="rows-per-page" className="text-sm font-medium">
                Rows per page
              </Label>
              <Select
                value={`${table.getState().pagination.pageSize}`}
                onValueChange={(value) => table.setPageSize(Number(value))}
              >
                <SelectTrigger size="sm" className="w-20" id="rows-per-page">
                  <SelectValue
                    placeholder={table.getState().pagination.pageSize}
                  />
                </SelectTrigger>
                <SelectContent side="top">
                  {[10, 20, 30, 40, 50].map((s) => (
                    <SelectItem key={s} value={`${s}`}>
                      {s}
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
            </div>
            <div className="flex w-fit items-center justify-center text-sm font-medium">
              Page {table.getState().pagination.pageIndex + 1} of{" "}
              {table.getPageCount()}
            </div>
            <div className="ml-auto flex items-center gap-2 lg:ml-0">
              <Button
                variant="outline"
                className="hidden h-8 w-8 p-0 lg:flex"
                onClick={() => table.setPageIndex(0)}
                disabled={!table.getCanPreviousPage()}
              >
                <span className="sr-only">Go to first page</span>
                <IconChevronsLeft />
              </Button>
              <Button
                variant="outline"
                className="size-8"
                size="icon"
                onClick={() => table.previousPage()}
                disabled={!table.getCanPreviousPage()}
              >
                <span className="sr-only">Go to previous page</span>
                <IconChevronLeft />
              </Button>
              <Button
                variant="outline"
                className="size-8"
                size="icon"
                onClick={() => table.nextPage()}
                disabled={!table.getCanNextPage()}
              >
                <span className="sr-only">Go to next page</span>
                <IconChevronRight />
              </Button>
              <Button
                variant="outline"
                className="hidden size-8 lg:flex"
                size="icon"
                onClick={() => table.setPageIndex(table.getPageCount() - 1)}
                disabled={!table.getCanNextPage()}
              >
                <span className="sr-only">Go to last page</span>
                <IconChevronsRight />
              </Button>
            </div>
          </div>
        </div>
      ) : null}
    </>
  )

  if (!tabs || tabs.length === 0) {
    return (
      <div
        data-testid={testId}
        className="flex w-full flex-col gap-4 px-4 lg:px-6"
      >
        {toolbar ? (
          <div className="flex items-center justify-end">{toolbar}</div>
        ) : null}
        {body}
      </div>
    )
  }

  return (
    <Tabs
      value={tabValue}
      onValueChange={onTabChange}
      data-testid={testId}
      className="w-full flex-col justify-start gap-6"
    >
      <div className="flex items-center justify-between gap-2 px-4 lg:px-6">
        <TabsList className="flex">
          {tabs.map((t) => (
            <TabsTrigger key={t.value} value={t.value}>
              {t.label}
            </TabsTrigger>
          ))}
        </TabsList>
        {toolbar ? <div className="flex items-center gap-2">{toolbar}</div> : null}
      </div>
      <TabsContent
        value={tabValue}
        className="relative flex flex-col gap-4 overflow-auto px-4 lg:px-6"
      >
        {body}
      </TabsContent>
    </Tabs>
  )
}
