/**
 * React-Query bindings for the §7.4 Product & Manufacturing surface
 * (DOCS/ideas/product-manufacturing.md). Mirrors the projects hooks
 * file: a stable key factory + thin query/mutation hooks with the
 * same cache discipline (30s lists, 15s detail, invalidate-on-write).
 *
 * Cache keys live under `["products", …]` / `["parties", …]` so a
 * logout flush clears them alongside the other section caches.
 */

import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";

import { api } from "../api/client.js";
import type { ProjectDto } from "../api/client.js";
import type {
  ArchivePartyRequest,
  ArchiveProductRequest,
  ArchiveReleaseRequest,
  CreateCustomerRequest,
  CreateManualRequest,
  CreatePartyRequest,
  CreateProductRequest,
  CreateReleaseRequest,
  CreateRevisionRequest,
  CustomerDto,
  CustomerListResponse,
  ListPartiesQuery,
  ListProductsQuery,
  ManualDto,
  ManualRevisionDto,
  PartyDto,
  PartyListResponse,
  PatchCustomerRequest,
  PatchPartyRequest,
  PatchProductRequest,
  PatchReleaseRequest,
  ProductDocumentDto,
  ProductDto,
  ProductListResponse,
  ProductReleaseDto,
} from "../api/schemas/products.js";

/** Stable cache keys — the invalidation surface for the mutation
 *  hooks below. */
export const productsKeys = {
  list: (q: ListProductsQuery) => ["products", "list", q] as const,
  count: (status: ProductDto["status"]) =>
    ["products", "count", status] as const,
  detail: (id: string) => ["products", "detail", id] as const,
  projects: (productId: string) =>
    ["products", "projects", productId] as const,
  productsForProject: (projectId: string) =>
    ["products", "for-project", projectId] as const,
  documents: (productId: string) =>
    ["products", "documents", productId] as const,
  manuals: (productId: string) => ["products", "manuals", productId] as const,
  revisions: (productId: string, manualId: string) =>
    ["products", "revisions", productId, manualId] as const,
  releases: (productId: string) =>
    ["products", "releases", productId] as const,
};

/** Party kind discriminator — the three list/edit surfaces share one
 *  component and pick the API method off this. */
export type PartyKind = "customers" | "manufacturers" | "suppliers";

export const partiesKeys = {
  list: (kind: PartyKind, q: ListPartiesQuery) =>
    ["parties", kind, "list", q] as const,
  detail: (kind: PartyKind, id: string) =>
    ["parties", kind, "detail", id] as const,
};

// ---------------------------------------------------------------------------
// Products — read
// ---------------------------------------------------------------------------

/** Full row fetch for the products hub. */
export function useProducts(q: ListProductsQuery) {
  return useQuery<ProductListResponse>({
    queryKey: productsKeys.list(q),
    queryFn: () => api.listProducts(q),
    staleTime: 30_000,
  });
}

/** Single product read for the detail page. Returns `null` when the
 *  product does not exist so the page renders a clean "not found". */
export function useProduct(id: string | null) {
  return useQuery<ProductDto | null>({
    queryKey: id ? productsKeys.detail(id) : ["products", "detail", "(none)"],
    queryFn: () => (id ? api.getProduct(id) : Promise.resolve(null)),
    enabled: !!id,
    staleTime: 15_000,
  });
}

// ---------------------------------------------------------------------------
// Products — write
// ---------------------------------------------------------------------------

function invalidateProductsRoot(qc: ReturnType<typeof useQueryClient>): void {
  qc.invalidateQueries({ queryKey: ["products"] });
}

export function useCreateProduct() {
  const qc = useQueryClient();
  return useMutation<ProductDto, Error, CreateProductRequest>({
    mutationFn: (body) => api.createProduct(body),
    onSuccess: () => invalidateProductsRoot(qc),
  });
}

export function usePatchProduct(productId: string) {
  const qc = useQueryClient();
  return useMutation<ProductDto, Error, PatchProductRequest>({
    mutationFn: (body) => api.patchProduct(productId, body),
    onSuccess: () => {
      qc.invalidateQueries({ queryKey: productsKeys.detail(productId) });
      invalidateProductsRoot(qc);
    },
  });
}

export function useArchiveProduct(productId: string) {
  const qc = useQueryClient();
  return useMutation<ProductDto, Error, ArchiveProductRequest>({
    mutationFn: (body) => api.archiveProduct(productId, body),
    onSuccess: () => {
      qc.invalidateQueries({ queryKey: productsKeys.detail(productId) });
      invalidateProductsRoot(qc);
    },
  });
}

// ---------------------------------------------------------------------------
// Product ↔ project links
// ---------------------------------------------------------------------------

/** Projects linked to a product (Projects tab on the product page). */
export function useProductProjects(productId: string | null) {
  return useQuery<ProjectDto[]>({
    queryKey: productId
      ? productsKeys.projects(productId)
      : ["products", "projects", "(none)"],
    queryFn: () =>
      productId ? api.listProductProjects(productId) : Promise.resolve([]),
    enabled: !!productId,
    staleTime: 30_000,
  });
}

/** Products linked to a project (Products panel on the project page). */
export function useProjectProducts(projectId: string | null) {
  return useQuery<ProductDto[]>({
    queryKey: projectId
      ? productsKeys.productsForProject(projectId)
      : ["products", "for-project", "(none)"],
    queryFn: () =>
      projectId ? api.listProjectProducts(projectId) : Promise.resolve([]),
    enabled: !!projectId,
    staleTime: 30_000,
  });
}

export function useLinkProductProject() {
  const qc = useQueryClient();
  return useMutation<void, Error, { productId: string; projectId: string }>({
    mutationFn: ({ productId, projectId }) =>
      api.linkProductProject(productId, projectId),
    onSuccess: (_, { productId, projectId }) => {
      qc.invalidateQueries({ queryKey: productsKeys.projects(productId) });
      qc.invalidateQueries({
        queryKey: productsKeys.productsForProject(projectId),
      });
    },
  });
}

export function useUnlinkProductProject() {
  const qc = useQueryClient();
  return useMutation<void, Error, { productId: string; projectId: string }>({
    mutationFn: ({ productId, projectId }) =>
      api.unlinkProductProject(productId, projectId),
    onSuccess: (_, { productId, projectId }) => {
      qc.invalidateQueries({ queryKey: productsKeys.projects(productId) });
      qc.invalidateQueries({
        queryKey: productsKeys.productsForProject(projectId),
      });
    },
  });
}

// ---------------------------------------------------------------------------
// Product documents
// ---------------------------------------------------------------------------

export function useProductDocuments(productId: string | null) {
  return useQuery<ProductDocumentDto[]>({
    queryKey: productId
      ? productsKeys.documents(productId)
      : ["products", "documents", "(none)"],
    queryFn: () =>
      productId ? api.listProductDocuments(productId) : Promise.resolve([]),
    enabled: !!productId,
    staleTime: 30_000,
  });
}

export function useUploadProductDocument(productId: string) {
  const qc = useQueryClient();
  return useMutation<
    ProductDocumentDto,
    Error,
    { file: File; title?: string; doc_type?: string; notes?: string }
  >({
    mutationFn: ({ file, ...fields }) =>
      api.uploadProductDocument(productId, file, fields),
    onSuccess: () => {
      qc.invalidateQueries({ queryKey: productsKeys.documents(productId) });
    },
  });
}

export function useDeleteProductDocument(productId: string) {
  const qc = useQueryClient();
  return useMutation<void, Error, string>({
    mutationFn: (documentId) =>
      api.deleteProductDocument(productId, documentId),
    onSuccess: () => {
      qc.invalidateQueries({ queryKey: productsKeys.documents(productId) });
    },
  });
}

// ---------------------------------------------------------------------------
// Manuals + revisions
// ---------------------------------------------------------------------------

export function useProductManuals(productId: string | null) {
  return useQuery<ManualDto[]>({
    queryKey: productId
      ? productsKeys.manuals(productId)
      : ["products", "manuals", "(none)"],
    queryFn: () =>
      productId ? api.listProductManuals(productId) : Promise.resolve([]),
    enabled: !!productId,
    staleTime: 30_000,
  });
}

export function useCreateProductManual(productId: string) {
  const qc = useQueryClient();
  return useMutation<ManualDto, Error, CreateManualRequest>({
    mutationFn: (body) => api.createProductManual(productId, body),
    onSuccess: () => {
      qc.invalidateQueries({ queryKey: productsKeys.manuals(productId) });
    },
  });
}

export function useManualRevisions(
  productId: string | null,
  manualId: string | null,
) {
  return useQuery<ManualRevisionDto[]>({
    queryKey:
      productId && manualId
        ? productsKeys.revisions(productId, manualId)
        : ["products", "revisions", "(none)"],
    queryFn: () =>
      productId && manualId
        ? api.listManualRevisions(productId, manualId)
        : Promise.resolve([]),
    enabled: !!productId && !!manualId,
    staleTime: 15_000,
  });
}

export function useCreateManualRevision(productId: string, manualId: string) {
  const qc = useQueryClient();
  return useMutation<ManualRevisionDto, Error, CreateRevisionRequest>({
    mutationFn: (body) => api.createManualRevision(productId, manualId, body),
    onSuccess: () => {
      qc.invalidateQueries({
        queryKey: productsKeys.revisions(productId, manualId),
      });
      qc.invalidateQueries({ queryKey: productsKeys.manuals(productId) });
    },
  });
}

export function usePublishManualRevision(productId: string, manualId: string) {
  const qc = useQueryClient();
  return useMutation<ManualRevisionDto, Error, string>({
    mutationFn: (revisionId) =>
      api.publishManualRevision(productId, manualId, revisionId),
    onSuccess: () => {
      qc.invalidateQueries({
        queryKey: productsKeys.revisions(productId, manualId),
      });
      qc.invalidateQueries({ queryKey: productsKeys.manuals(productId) });
    },
  });
}

// ---------------------------------------------------------------------------
// Parties (customers / manufacturers / suppliers)
// ---------------------------------------------------------------------------

/** List the chosen party kind. Customers carry the extra
 *  `account_ref` field; the response shapes are otherwise identical
 *  (`{rows,total,limit,offset}`), so the component treats the rows
 *  as `PartyDto` and reads `account_ref` defensively. */
export function useParties(kind: PartyKind, q: ListPartiesQuery) {
  return useQuery<PartyListResponse | CustomerListResponse>({
    queryKey: partiesKeys.list(kind, q),
    queryFn: () => {
      switch (kind) {
        case "customers":
          return api.listCustomers(q);
        case "manufacturers":
          return api.listManufacturers(q);
        case "suppliers":
          return api.listSuppliers(q);
      }
    },
    staleTime: 30_000,
  });
}

/** Single customer read for the customer-detail page. */
export function useCustomer(id: string | null) {
  return useQuery<CustomerDto | null>({
    queryKey: id
      ? partiesKeys.detail("customers", id)
      : ["parties", "customers", "detail", "(none)"],
    queryFn: () => (id ? api.getCustomer(id) : Promise.resolve(null)),
    enabled: !!id,
    staleTime: 15_000,
  });
}

function invalidatePartiesRoot(
  qc: ReturnType<typeof useQueryClient>,
  kind: PartyKind,
): void {
  qc.invalidateQueries({ queryKey: ["parties", kind] });
}

export function useCreateParty(kind: PartyKind) {
  const qc = useQueryClient();
  return useMutation<
    PartyDto | CustomerDto,
    Error,
    CreatePartyRequest | CreateCustomerRequest
  >({
    mutationFn: (body) => {
      switch (kind) {
        case "customers":
          return api.createCustomer(body as CreateCustomerRequest);
        case "manufacturers":
          return api.createManufacturer(body);
        case "suppliers":
          return api.createSupplier(body);
      }
    },
    onSuccess: () => invalidatePartiesRoot(qc, kind),
  });
}

export function usePatchParty(kind: PartyKind) {
  const qc = useQueryClient();
  return useMutation<
    PartyDto | CustomerDto,
    Error,
    { id: string; body: PatchPartyRequest | PatchCustomerRequest }
  >({
    mutationFn: ({ id, body }) => {
      switch (kind) {
        case "customers":
          return api.patchCustomer(id, body as PatchCustomerRequest);
        case "manufacturers":
          return api.patchManufacturer(id, body);
        case "suppliers":
          return api.patchSupplier(id, body);
      }
    },
    onSuccess: (_, { id }) => {
      qc.invalidateQueries({ queryKey: partiesKeys.detail(kind, id) });
      invalidatePartiesRoot(qc, kind);
    },
  });
}

export function useArchiveParty(kind: PartyKind) {
  const qc = useQueryClient();
  return useMutation<
    PartyDto | CustomerDto,
    Error,
    { id: string; body: ArchivePartyRequest }
  >({
    mutationFn: ({ id, body }) => {
      switch (kind) {
        case "customers":
          return api.archiveCustomer(id, body);
        case "manufacturers":
          return api.archiveManufacturer(id, body);
        case "suppliers":
          return api.archiveSupplier(id, body);
      }
    },
    onSuccess: (_, { id }) => {
      qc.invalidateQueries({ queryKey: partiesKeys.detail(kind, id) });
      invalidatePartiesRoot(qc, kind);
    },
  });
}

// ---------------------------------------------------------------------------
// Firmware & Software releases
// ---------------------------------------------------------------------------

export function useProductReleases(productId: string | null) {
  return useQuery<ProductReleaseDto[]>({
    queryKey: productId
      ? productsKeys.releases(productId)
      : ["products", "releases", "(none)"],
    queryFn: () =>
      productId ? api.listProductReleases(productId) : Promise.resolve([]),
    enabled: !!productId,
    staleTime: 30_000,
  });
}

export function useCreateRelease(productId: string) {
  const qc = useQueryClient();
  return useMutation<ProductReleaseDto, Error, CreateReleaseRequest>({
    mutationFn: (body) => api.createProductRelease(productId, body),
    onSuccess: () => {
      qc.invalidateQueries({ queryKey: productsKeys.releases(productId) });
    },
  });
}

export function usePatchRelease(productId: string) {
  const qc = useQueryClient();
  return useMutation<
    ProductReleaseDto,
    Error,
    { releaseId: string; body: PatchReleaseRequest }
  >({
    mutationFn: ({ releaseId, body }) =>
      api.patchProductRelease(productId, releaseId, body),
    onSuccess: () => {
      qc.invalidateQueries({ queryKey: productsKeys.releases(productId) });
    },
  });
}

export function useArchiveRelease(productId: string) {
  const qc = useQueryClient();
  return useMutation<
    ProductReleaseDto,
    Error,
    { releaseId: string; body: ArchiveReleaseRequest }
  >({
    mutationFn: ({ releaseId, body }) =>
      api.archiveProductRelease(productId, releaseId, body),
    onSuccess: () => {
      qc.invalidateQueries({ queryKey: productsKeys.releases(productId) });
    },
  });
}
