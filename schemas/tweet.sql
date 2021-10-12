CREATE TABLE tweet (
    id INTEGER NOT NULL PRIMARY KEY,
    twitter_id INTEGER NOT NULL,
    parent_twitter_id INTEGER NULL,
    ts INTEGER NOT NULL,
    user_twitter_id INTEGER NOT NULL,
    content TEXT NOT NULL
);
CREATE INDEX tweet_twitter_id_index ON tweet (twitter_id);
CREATE INDEX tweet_parent_twitter_id ON tweet (parent_twitter_id);
CREATE INDEX tweet_user_twitter_id ON tweet (user_twitter_id);

CREATE TABLE user (
    id INTEGER NOT NULL PRIMARY KEY,
    twitter_id INTEGER NOT NULL,
    screen_name TEXT NOT NULL,
    name TEXT NOT NULL
);
CREATE INDEX user_twitter_id ON user (twitter_id);

CREATE TABLE file (
    id INTEGER NOT NULL PRIMARY KEY,
    digest TEXT UNIQUE NOT NULL,
    primary_twitter_id INTEGER NULL
);

CREATE TABLE tweet_file (
    tweet_id INTEGER NOT NULL,
    file_id INTEGER NOT NULL,
    user_id INTEGER NOT NULL,
    FOREIGN KEY (tweet_id) REFERENCES tweet (id),
    FOREIGN KEY (file_id) REFERENCES file (id)
);
CREATE INDEX tweet_file_tweet_id_index ON tweet_file (tweet_id);
CREATE INDEX tweet_file_file_id_index ON tweet_file (file_id);
CREATE INDEX tweet_file_user_id_index ON tweet_file (user_id);