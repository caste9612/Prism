-- Schema version 9: names of the servers of network shares reached by IP address (REVIEW D59).
-- A NAS can get another address from the router: its name finds it again.
CREATE TABLE share_servers (
  address TEXT    PRIMARY KEY,   -- the server as written in the UNC paths, e.g. "192.168.1.65"
  name    TEXT    NOT NULL,      -- its host name, e.g. "ZimaOS.local"
  seen_at INTEGER NOT NULL       -- milliseconds since 1970
) STRICT;
