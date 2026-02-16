import type { HttpMethod } from "./http.js";

export interface RouteIR {
  method: HttpMethod;
  path: string;
  handlerRef: string;
  middlewareRefs?: string[];
}
