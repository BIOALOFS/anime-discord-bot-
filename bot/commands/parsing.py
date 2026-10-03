def profile_arguments(arguments,mentions,author):
    target=mentions[0] if mentions else author
    tokens=[token for token in arguments.split() if not token.startswith("<@")]
    if len(tokens)>1 or (tokens and not tokens[0].isdigit()):raise ValueError("use !profile, !profile 5, !profile @user, or !profile @user 5")
    position=int(tokens[0]) if tokens else 1
    if position<1:raise ValueError("position must be at least 1")
    return target,position

def find_arguments(arguments,mentions,author):
    target=mentions[0] if mentions else author
    query=" ".join(token for token in arguments.split() if not token.startswith("<@"))
    if not query:raise ValueError("a search query is required")
    return target,query
