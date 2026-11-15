# Contract Change Requests

Changes to a contract (PROCESS.md P3) after its owning subphase merged (rule R2). Append only.
Additive changes merge immediately as a contract-only PR; breaking changes merge only between waves.

Format:

```
## CR-<NNN> — <contract>: <title>
Requested by: <work item>   Kind: additive | breaking   Status: open | merged (<PR>) | rejected
Change: <exact types/fields/schema>
Why: <the requirement or finding that needs it>
Consumers affected: <modules, bindings, artifacts>
```
