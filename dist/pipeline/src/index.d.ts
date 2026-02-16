export interface PipelineOptions {
    log?: (message: string) => void;
}
export declare function runPipeline(cwd: string, options?: PipelineOptions): Promise<void>;
