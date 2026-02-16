export interface RouteIR {
    method: 'GET' | 'POST' | 'PUT' | 'DELETE';
    path: string;
    handler: string;
}
export interface ProgramIR {
    routes: RouteIR[];
    warnings: string[];
}
