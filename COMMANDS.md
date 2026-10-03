# Command reference

Arabic is the default response language; use `!language en`. Prefix multi-word values must be quoted, for example `!move "Sailor Moon" 2`. Stable IDs are the safest disambiguator. Ambiguous resolution returns choices instead of choosing the first match.

| Area | Prefix | Slash | Purpose / example |
|---|---|---|---|
| Collection | `!add <name-or-id>` | `/add character:` | Add one reviewed catalog identity; `!add <uuid>` |
| | `!remove <name-or-position>` | `/remove character:` | Remove while recording undo |
| | `!move <character> <position>` | `/move character: position:` | Transactional reorder |
| | `!profile [@user] [position]` | `/profile user: position:` | Browse with owner-locked controls |
| | `!random` | `/random` | Random collection entry |
| | `!find [@user] <query>` | `/find query: user:` | Collection search |
| Search | `!p <query>` / `!sp <query>` | `/p` / `/sp` | Character results without/with images |
| | `!sa <work>` / `!sap <work>` | `/sa` / `/sap` | Work cast without/with images |
| Images | `!pi <character> <URL>` or attachment | `/image add character: url:/attachment:` | Exactly one source; retained pending content review |
| | `!images <character>` | `/image list character:` | Visually browse the approved deterministic gallery |
| | `!pic <character> <n>` | `/image select character: number:` | Persist an approved catalog or personal default |
| | `!piremove <character> <n>` | `/image remove character: number:` | Remove personal image only |
| | `!pireset <character>` | `/image reset character:` | Catalog default; gallery remains |
| Metadata | `!nick` / `!unnick` | `/nickname set` / `/nickname reset` | Set/reset personal nickname; flat aliases remain |
| | `!note` / `!unnote` | `/note set` / `/note reset` | Set/reset personal note |
| Appearance | `!avatar`, `!banner` | `/settings avatar`, `/settings banner` | Exactly one HTTPS URL/attachment, pending review |
| | `!bio`, `!title`, `!color`, `!language` | `/settings bio|title|color|language` | Profile settings; flat aliases remain |
| | `!settings` | `/settings view` | View settings |
| | `!reset <option>` | `/settings reset option:` | Reset avatar/banner/bio/title/color/language/all; all requires confirmation |
| Utilities | `!custom` | `/custom` | Guided personal pending entry |
| | `!editcustom` | `/editcustom` | Owner-only custom edit and re-review |
| | `!undo` | `/undo` | Restore last removal and metadata |
| | `!compare @user` | `/compare user:` | Jaccard intersection/union overlap |
| | `!export` | `/export` | Versioned JSON; URLs are not backups |
| | `!report <character> <reason>` | `/report` | `metadata`, `duplicate`, `broken_image`, `excluded_content` |
| | `!help` | `/help` | Navigable topic index |
| Server administration | `!setup #channel` | `/setup channel:` | Initial setup; replacing existing setup requires confirmation |
| | `!channel add #channel` | `/channel add channel:` | Add a channel; confirms before leaving all-channel mode |
| | `!channel remove #channel` | `/channel remove channel:` | Remove a channel, but never the final restricted channel |
| | `!channel list` | `/channel list` | Show restricted/all/unconfigured status |
| | `!channel all` | `/channel all` | Explicit, confirmed all-channel mode; Discord permissions still apply |

Examples: `!setup #characters`, `!channel add #bot-commands`, `/channel remove channel:#old-bot`. These commands are server-only and require the owner, Manage Server, a global trusted user, or a user listed for that server. Server-specific trust never grants catalog access. Trust is edited only in `trusted_users.json`.

Discord slash groups use the documented logical names where platform registration permits; compact hybrid aliases call the same service methods. Prefix attachment commands have slash adapters so both interfaces apply identical ownership, validation, and persistence rules.

`!profile` accepts no argument, a numeric position, a mention, or a mention followed by a position. `!find` accepts an optional leading mention followed by an unquoted multiword query. Visual searches use owner-bound pagination and Add controls; work search always asks which production/adaptation was intended. Provider coverage and pagination limits are reported rather than described as exhaustive.

Exports use schema version 2 and include descriptive names, works, provider references, personal metadata, and selected image references. They are **not** a complete backup or a tested import/restore format.
