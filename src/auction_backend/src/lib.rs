use candid::{CandidType, Decode, Deserialize, Encode};
use chrono::{DateTime, TimeZone, Utc};
use ic_stable_structures::memory_manager::{MemoryId, MemoryManager, VirtualMemory};
use ic_stable_structures::{BoundedStorable, DefaultMemoryImpl, StableBTreeMap, Storable};
use std::{borrow::Cow, cell::RefCell, u32};

type Memory = VirtualMemory<DefaultMemoryImpl>;

const MAX_VALUE_SIZE: u32 = 500;

#[derive(CandidType, Deserialize)]
struct Bid {
    auction: u64,
    currency: String,
    amount: u32,
    owner: candid::Principal,
}

#[derive(CandidType, Deserialize)]
struct CreateBid {
    currency: String,
    amount: u32,
}

#[derive(CandidType, Deserialize)]
struct Auction {
    title: String,
    detail: String,
    currency: String,
    amount: u32,
    end_time: String,
    bids: Vec<Bid>,
    main_bid: Bid,
    owner: candid::Principal,
    new_owner: candid::Principal,
    is_active: bool,
}

#[derive(CandidType, Deserialize)]
struct CreateAuction {
    title: String,
    detail: String,
    currency: String,
    amount: u32,
    end_time: String,
    is_active: bool,
}

#[derive(CandidType)]
enum BidError {
    _AlreadyMaxBid,
    AuctionIsNotActive,
    NoSuchAuction,
    BidAmountLessThanCurrent,
    UpdateError,
    OwnerCantBid,
}

#[derive(CandidType)]
enum AuctionError {
    AuctionIsNotActive,
    NoSuchAuction,
    AccessRejected,
    UpdateError,
    InvalidDate,
}

impl Storable for Auction {
    fn to_bytes(&self) -> Cow<[u8]> {
        Cow::Owned(Encode!(self).unwrap())
    }

    fn from_bytes(bytes: Cow<[u8]>) -> Self {
        Decode!(bytes.as_ref(), Self).unwrap()
    }
}

impl BoundedStorable for Auction {
    const MAX_SIZE: u32 = MAX_VALUE_SIZE;
    const IS_FIXED_SIZE: bool = false;
}

thread_local! {
    static MEMORY_MANAGER: RefCell<MemoryManager<DefaultMemoryImpl>> = RefCell::new(MemoryManager::init(DefaultMemoryImpl::default()));

    static AUCTION_MAP: RefCell<StableBTreeMap<u64, Auction, Memory>> = RefCell::new(StableBTreeMap::init(
        MEMORY_MANAGER.with(|m| m.borrow().get(MemoryId::new(0))),
    ));

    static BIDS_MAP: RefCell<StableBTreeMap<u64, Auction, Memory>> = RefCell::new(StableBTreeMap::init(
        MEMORY_MANAGER.with(|m| m.borrow().get(MemoryId::new(1))),
    ));
}

fn validate_iso8601_datetime(datetime: &str) -> Result<(), String> {
    match DateTime::parse_from_rfc3339(datetime) {
        Ok(parsed_datetime) => {
            // let current_datetime = Utc::now();
            // Due to chrono error I used static Date, After fixing Code can be changed with above code for dynamic Date.
            let current_datetime = Utc.ymd(2023, 8, 27).and_hms(23, 59, 59);

            if parsed_datetime > current_datetime {
                Ok(())
            } else {
                Err("The datetime should be in the future.".to_string())
            }
        }
        Err(_) => Err("Invalid ISO 8601 datetime format.".to_string()),
    }
}

#[ic_cdk::query]
fn get_auction(key: u64) -> Option<Auction> {
    AUCTION_MAP.with(|p| p.borrow().get(&key))
}

#[ic_cdk::query]
fn get_auction_list() -> Option<Vec<Auction>> {
    AUCTION_MAP.with(|map_ref| {
        let map = map_ref.borrow();
        map.iter().map(|(_key, value)| Some(value)).collect()
    })
}

#[ic_cdk::query]
fn get_auctions_number() -> u64 {
    AUCTION_MAP.with(|p| p.borrow().len())
}

#[ic_cdk::query]
fn get_most_bidded_auction() -> Option<Auction> {
    AUCTION_MAP.with(|map_ref| {
        let map = map_ref.borrow();
        let mut most_bidded_auction: Option<Auction> = None;
        let mut max_bids = 0;

        for (_key, value) in map.iter() {
            let num_bids = value.bids.len();
            if num_bids > max_bids {
                max_bids = num_bids;
                most_bidded_auction = Some(value);
            }
        }

        most_bidded_auction
    })
}

#[ic_cdk::query]
fn get_auction_bids(auction_key: u64) -> Option<Vec<Bid>> {
    AUCTION_MAP.with(|map_ref| {
        let map = map_ref.borrow();
        let auction = match map.get(&auction_key) {
            Some(auction) => auction,
            None => return None, // Müzaye bulunamadı
        };

        Some(auction.bids) // Müzayenin tekliflerini kopyala ve döndür
    })
}

#[ic_cdk::update]
fn create_auction(key: u64, auction: CreateAuction) -> Result<(), AuctionError> {
    match validate_iso8601_datetime(&auction.end_time) {
        Ok(_) => println!("Datetime is valid."),
        Err(_err) => return Err(AuctionError::InvalidDate),
    }

    let value = Auction {
        title: auction.title,
        detail: auction.detail,
        currency: auction.currency,
        amount: auction.amount,
        end_time: auction.end_time,
        bids: Vec::new(),
        main_bid: Bid {
            auction: 0,
            currency: "".to_string(),
            amount: 0,
            owner: candid::Principal::anonymous(),
        },
        owner: ic_cdk::caller(),
        new_owner: candid::Principal::anonymous(),
        is_active: auction.is_active,
    };

    AUCTION_MAP.with(|p| {
        let result = p.borrow_mut().insert(key, value);

        match result {
            Some(_) => Ok(()),
            None => Err(AuctionError::UpdateError),
        }
    })
}

#[ic_cdk::update]
fn edit_auction(key: u64, auction: CreateAuction) -> Result<(), AuctionError> {
    AUCTION_MAP.with(|p| {
        let old_auction_opt = p.borrow().get(&key);
        let old_auction = match old_auction_opt {
            Some(value) => value,
            None => return Err(AuctionError::NoSuchAuction),
        };

        if !auction.is_active {
            return Err(AuctionError::AuctionIsNotActive);
        }

        if ic_cdk::caller() != old_auction.owner {
            return Err(AuctionError::AccessRejected);
        }

        let value = Auction {
            title: auction.title,
            detail: auction.detail,
            currency: auction.currency,
            amount: auction.amount,
            end_time: auction.end_time,
            ..old_auction
        };

        let result = p.borrow_mut().insert(key, value);

        match result {
            Some(_) => Ok(()),
            None => Err(AuctionError::UpdateError),
        }
    })
}

#[ic_cdk::update]
fn end_proposal(key: u64) -> Result<(), AuctionError> {
    AUCTION_MAP.with(|p| {
        let auction_opt = p.borrow().get(&key);
        let mut auction = match auction_opt {
            Some(value) => value,
            None => return Err(AuctionError::NoSuchAuction),
        };

        if ic_cdk::caller() != auction.owner {
            return Err(AuctionError::AccessRejected);
        }

        let mut max_bid_amount = 0;
        let mut max_bid_owner = candid::Principal::anonymous();

        for bid in &auction.bids {
            if bid.amount > max_bid_amount {
                max_bid_amount = bid.amount;
                max_bid_owner = bid.owner;
            }
        }

        auction.new_owner = max_bid_owner;

        auction.is_active = false;

        let result = p.borrow_mut().insert(key, auction);

        match result {
            Some(_) => Ok(()),
            None => Err(AuctionError::UpdateError),
        }
    })
}

#[ic_cdk::update]
fn insert_bid(auction_key: u64, bid: CreateBid) -> Result<(), BidError> {
    AUCTION_MAP.with(|auction_map_ref| {
        let mut auction_map = auction_map_ref.borrow_mut();

        let mut auction = match auction_map.get(&auction_key) {
            Some(auction) => auction,
            None => return Err(BidError::NoSuchAuction),
        };

        if !auction.is_active {
            return Err(BidError::AuctionIsNotActive);
        }

        if bid.amount <= auction.amount {
            return Err(BidError::BidAmountLessThanCurrent);
        }

        if bid.amount <= auction.main_bid.amount {
            return Err(BidError::BidAmountLessThanCurrent);
        }

        if ic_cdk::caller() == auction.main_bid.owner {
            return Err(BidError::OwnerCantBid);
        }

        auction.bids.push(Bid {
            auction: auction_key,
            currency: bid.currency.clone(),
            amount: bid.amount,
            owner: ic_cdk::caller(),
        });

        auction.main_bid = Bid {
            auction: auction_key,
            currency: bid.currency,
            amount: bid.amount,
            owner: ic_cdk::caller(),
        };

        auction.bids.sort_by(|a, b| b.amount.cmp(&a.amount));

        let result = auction_map.insert(auction_key, auction);

        match result {
            Some(_) => Ok(()),
            None => Err(BidError::UpdateError),
        }
    })
}
