import type {
  InlineConfig as TsdownInlineConfig,
  UserConfig as TsdownUserConfig,
} from "tsdown";

export type Arrayable<T> = T | T[];

type Awaitable<T> = T | Promise<T>;

export interface GoConfig {
  package?: string;
  module?: string;
  port?: number;
  strictSemantics?: boolean;
}

export type UserConfigFn = (
  inlineConfig: TsdownInlineConfig,
  context: { ci: boolean },
) => Awaitable<Arrayable<UserConfig>>;

export type UserConfigExport = Awaitable<Arrayable<UserConfig> | UserConfigFn>;

export interface UserConfig extends TsdownUserConfig {
  go?: GoConfig;
}
