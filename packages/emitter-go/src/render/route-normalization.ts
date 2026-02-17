import type { RouteIR } from "@tsgodown/ir-core";

export function normalizeHttpMethod(method: string): string {
  const normalized = method.trim().toUpperCase();
  return normalized.length > 0 ? normalized : "GET";
}

export function normalizeRoutePath(pathname: string): string {
  const trimmed = pathname.trim();
  if (trimmed.length === 0) {
    return "/";
  }
  return trimmed.startsWith("/") ? trimmed : `/${trimmed}`;
}

export function toServeMuxPath(pathname: string): string {
  return normalizeRoutePath(pathname).replaceAll(
    /:([A-Za-z_][A-Za-z0-9_]*)/g,
    "{$1}",
  );
}

export function toServeMuxPattern(route: RouteIR): string {
  return `${normalizeHttpMethod(route.method)} ${toServeMuxPath(route.path)}`;
}

export function extractPathParamNames(pathname: string): string[] {
  const names: string[] = [];
  const seen = new Set<string>();
  const normalized = normalizeRoutePath(pathname);

  for (const match of normalized.matchAll(/:([A-Za-z_][A-Za-z0-9_]*)/g)) {
    const name = match[1];
    if (!seen.has(name)) {
      seen.add(name);
      names.push(name);
    }
  }

  for (const match of normalized.matchAll(
    /\{([A-Za-z_][A-Za-z0-9_]*)(?:\.\.\.)?\}/g,
  )) {
    const name = match[1];
    if (!seen.has(name)) {
      seen.add(name);
      names.push(name);
    }
  }

  return names;
}
