CREATE TABLE tweet (
    id INTEGER NOT NULL PRIMARY KEY,
    twitter_id INTEGER NOT NULL,
    ts INTEGER NOT NULL,
    user_id INTEGER NOT NULL,
    user_screen_name TEXT NOT NULL,
    content TEXT NOT NULl    
);
CREATE INDEX tweet_twitter_id_index ON tweet (twitter_id);
CREATE INDEX tweet_user_screen_name_index ON tweet (user_screen_name);

CREATE TABLE digest (
    tweet_id INTEGER NOT NULL,
    value TEXT NOT NULL,
    url TEXT NOT NULL,
    PRIMARY KEY (tweet_id, value),
    FOREIGN KEY (tweet_id) REFERENCES tweet (id)
);