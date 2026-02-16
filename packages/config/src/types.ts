export type Arrayable<T> = T | T[];

export type UserConfigFn = (env: { mode: string }) =>
  | UserConfig
  | Promise<UserConfig>;
export type UserConfigExport = UserConfig | UserConfig[] | UserConfigFn;

export interface UserConfig {
  entry?: string | string[] | Record<string, string>;
  outDir?: string;
  platform?: "node" | "browser" | "neutral";
  treeshake?: boolean | { moduleSideEffects?: boolean };
  define?: Record<string, string>;
  alias?: Record<string, string>;
  plugins?: Arrayable<unknown>;
  onSuccess?: () => void | Promise<void>;

  // tsgodown extensions
  go?: {
    package?: string;
    module?: string;
    port?: number;
    strictSemantics?: boolean;
  };
  fastify?: {
    detectPlugins?: boolean;
    routeMode?: "direct" | "register-aware";
  };
}
