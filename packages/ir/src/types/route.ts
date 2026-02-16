export interface RouteIR {
  method: "GET" | "POST" | "PUT" | "DELETE";
  path: string;
  handler: string;
}
