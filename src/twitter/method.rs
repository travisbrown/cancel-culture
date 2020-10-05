use egg_mode::service::{
    DirectMethod, ListMethod, PlaceMethod, SearchMethod, ServiceMethod, TweetMethod, UserMethod,
};

#[derive(Debug, Eq, Hash, PartialEq)]
pub enum Method {
    ///A method from the `direct` module.
    Direct(DirectMethod),
    ///A method from the `list` module.
    List(ListMethod),
    ///A method from the `place` module.
    Place(PlaceMethod),
    ///A method from the `search` module.
    Search(SearchMethod),
    ///A method from the `service` module.
    Service(ServiceMethod),
    ///A method from the `tweet` module.
    Tweet(TweetMethod),
    ///A method from the `user` module.
    User(UserMethod),
}

impl Method {
    pub const USER_FOLLOWED_IDS: &'static Method = &Method::User(UserMethod::FriendsIds);
    pub const USER_FOLLOWER_IDS: &'static Method = &Method::User(UserMethod::FollowersIds);
    pub const USER_LOOKUP: &'static Method = &Method::User(UserMethod::Lookup);
}

impl From<DirectMethod> for Method {
    fn from(m: DirectMethod) -> Self {
        Method::Direct(m)
    }
}

impl From<ListMethod> for Method {
    fn from(m: ListMethod) -> Self {
        Method::List(m)
    }
}

impl From<PlaceMethod> for Method {
    fn from(m: PlaceMethod) -> Self {
        Method::Place(m)
    }
}

impl From<SearchMethod> for Method {
    fn from(m: SearchMethod) -> Self {
        Method::Search(m)
    }
}

impl From<ServiceMethod> for Method {
    fn from(m: ServiceMethod) -> Self {
        Method::Service(m)
    }
}

impl From<TweetMethod> for Method {
    fn from(m: TweetMethod) -> Self {
        Method::Tweet(m)
    }
}

impl From<UserMethod> for Method {
    fn from(m: UserMethod) -> Self {
        Method::User(m)
    }
}
