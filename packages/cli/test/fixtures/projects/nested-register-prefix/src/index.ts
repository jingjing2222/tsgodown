type NestedApp = {
  register: (
    plugin: (app: NestedApp) => Promise<void> | void,
    opts?: unknown,
  ) => void;
  get?: (path: string, handler: () => unknown) => void;
};

const app: NestedApp = {
  register: (_plugin, _opts) => undefined,
};

app.register(
  async (root) => {
    root.register(
      async (v1) => {
        v1.get?.("/health", () => ({ ok: true }));
      },
      { prefix: "/v1" },
    );
  },
  { prefix: "/api" },
);
