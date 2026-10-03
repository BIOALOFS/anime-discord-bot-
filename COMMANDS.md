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
| Images | `!pi <character> <URL>` or attachment | `/image add` | Exactly one image source; saves default |
| | `!images <character>` | `/images` | List gallery |
| | `!pic <character> <n>` | `/pic` | Persist selected default |
| | `!piremove <character> <n>` | `/piremove` | Remove personal image only |
| | `!pireset <character>` | `/pireset` | Catalog default; gallery remains |
| Metadata | `!nick` / `!unnick` | `/nick` / `/unnick` | Set/reset personal nickname |
| | `!note` / `!unnote` | `/note` / `/unnote` | Set/reset personal note |
| Appearance | `!avatar`, `!banner` | `/settings avatar`, `/settings banner` | Reviewed HTTPS URL/attachment |
| | `!bio`, `!title`, `!color`, `!language` | Same slash names | Profile settings |
| | `!settings` | `/settings` | View settings |
| | `!reset <option>` | `/reset option:` | Reset avatar/banner/bio/title/color/language/all; all requires confirmation |
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
