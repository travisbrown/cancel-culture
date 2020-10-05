CREATE TABLE user (
    id INTEGER NOT NULL PRIMARY KEY,
    ts INTEGER NOT NULL
);
CREATE TABLE screen_name (
    id INTEGER NOT NULL PRIMARY KEY,
    value TEXT NOT NULL UNIQUE
);
CREATE TABLE user_observation (
    user_id INTEGER NOT NULL,
    ts INTEGER NOT NULL DEFAULT (strftime('%s','now')),
    screen_name_id INTEGER NOT NULL,
    follower_count INTEGER NOT NULL,
    following_count INTEGER NOT NULL,
    verified BOOLEAN NOT NULL,
    PRIMARY KEY (user_id, ts),
    FOREIGN KEY (user_id) REFERENCES user (id),
    FOREIGN KEY (screen_name_id) REFERENCES screen_name (id)
);
CREATE TABLE follow (
    follower_id INTEGER NOT NULL,
    followed_id INTEGER NOT NULL,
    ts INTEGER NOT NULL DEFAULT (strftime('%s','now')),
    PRIMARY KEY (follower_id, followed_id, ts)
);
CREATE INDEX follow_follower_id_index ON follow (follower_id);
CREATE INDEX follow_followed_id_index ON follow (followed_id);
CREATE TABLE unfollow (
    follower_id INTEGER NOT NULL,
    followed_id INTEGER NOT NULL,
    ts INTEGER NOT NULL DEFAULT (strftime('%s','now')),
    PRIMARY KEY (follower_id, followed_id, ts)
);
CREATE TABLE tweet (
    id INTEGER NOT NULL PRIMARY KEY,
    user_id INTEGER NOT NULL
);
CREATE INDEX tweet_user_id_index ON tweet (user_id);
CREATE TABLE tweet_data (
    tweet_id INTEGER NOT NULL PRIMARY KEY,
    created INTEGER NOT NULL,
    content TEXT NOT NULL,
    reply_to INTEGER,
    retweet_of INTEGER,
    quoting INTEGER,
    FOREIGN KEY (tweet_id) REFERENCES tweet (id)
);
CREATE TABLE tweet_observation (
    tweet_id INTEGER NOT NULL,
    ts INTEGER NOT NULL DEFAULT (strftime('%s','now')),
    retweet_count INTEGER NOT NULL,
    favorite_count INTEGER NOT NULL,
    PRIMARY KEY (tweet_id, ts),
    FOREIGN KEY (tweet_id) REFERENCES tweet (id)
);
