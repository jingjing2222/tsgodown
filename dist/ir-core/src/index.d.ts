export type HttpMethod = "GET" | "POST" | "PUT" | "DELETE" | "PATCH";
export interface ModuleIR {
    id: string;
    sourcePath: string;
    exports: string[];
    imports: Array<{
        spec: string;
        kind: "esm" | "cjs";
        resolved?: string;
    }>;
}
export interface RouteIR {
    method: HttpMethod;
    path: string;
    handlerRef: string;
    middlewareRefs?: string[];
}
export interface HandlerIR {
    id: string;
    params: Array<{
        name: string;
        role: "request" | "response" | "next" | "custom";
    }>;
    bodyRef?: string;
    async: boolean;
}
export interface DiagnosticIR {
    level: "error" | "warn" | "info";
    code: string;
    message: string;
    source?: {
        file: string;
        line?: number;
        column?: number;
        viaSourceMap?: boolean;
    };
}
export interface ProgramIR {
    modules: ModuleIR[];
    routes: RouteIR[];
    handlers: HandlerIR[];
    diagnostics: DiagnosticIR[];
}
