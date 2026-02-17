export type HandlerResponseMode =
  | "return"
  | "response-object"
  | "next-callback"
  | "unknown";

export interface HandlerIR {
  id: string;
  params: Array<{
    name: string;
    role: "request" | "response" | "next" | "custom";
  }>;
  bodyRef?: string;
  async: boolean;
  semantics?: {
    responseMode: HandlerResponseMode;
    requestParam?: string;
    responseParam?: string;
    usesStatus: boolean;
    usesBody: boolean;
    usesHeaders: boolean;
    usesJson: boolean;
  };
}
