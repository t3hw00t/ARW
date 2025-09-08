# arw-svc service layer

## gRPC vs GraphQL

- **gRPC (tonic)**
  - Contract-first API with protobuf and generated server/client code.
  - Efficient binary transport and native async support in Rust.
  - Fits existing Tower/Axum ecosystem used by the service.
- **GraphQL (async-graphql)**
  - Flexible queries and strong introspection support.
  - Higher runtime overhead and more complex authorization per field.
  - Client code generation is less standardized.

**Decision:** gRPC via `tonic` was selected for the internal service layer to provide a typed, efficient protocol that integrates with the existing stack.

The repository now contains a `Healthz` RPC definition and accompanying server/client code. The HTTP `/healthz` endpoint delegates to this gRPC method to illustrate the pattern for future handlers.
