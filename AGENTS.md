# ******

## Primary rule

You should almost never write code comments. Try use clear naming and module structure, plus leveraging the rust type system to make the code self-explanary. You should always try to make illegal things impossible to express, like using type-state pattern to eliminate illegal state transition. Only for those extremely tricky things you should write a concise comment to notice others. Doc comments are recommended, but they should be targeted on how to use the api and the common pitfalls when using them, and the doc comments should be concise.

Try you best to keep everything single-sourced, whatever it is code or doc.

Remember to update the doc to reflect the current codebase if you make some changes.

## Development

If you are writing a new feature, you should always search relevant code and try to reuse them when possible and doesn't contradiced with the primary rule.

## Exploring

When exploring the codebase, you should first evaluate its scale using tools like `cloc`, if it's huge, use subagents to do the exploration.
