use pgrx::*;
use vectorize_core::core::types::{
    IndexDist as CoreIndexDist, SimilarityAlg as CoreSimilarityAlg, TableMethod as CoreTableMethod,
};

use serde::{Deserialize, Serialize};

#[allow(non_camel_case_types)]
#[derive(Clone, Debug, Default, Serialize, Deserialize, PostgresEnum, PartialEq, Eq)]
pub enum TableMethod {
    append,
    #[default]
    join,
}

impl From<TableMethod> for CoreTableMethod {
    fn from(my_method: TableMethod) -> Self {
        match my_method {
            TableMethod::append => CoreTableMethod::append,
            TableMethod::join => CoreTableMethod::join,
        }
    }
}

// NOTE: re-implementing SimilarityAlg enum from vectorize_core because we need to derive the PostgresEnum trait on it here
// this Enum will be soon deprecated
#[allow(non_camel_case_types)]
#[derive(Clone, Debug, Serialize, Deserialize, PostgresEnum)]
//
// SimilarityAlg is now deprecated
//
pub enum SimilarityAlg {
    pgv_cosine_similarity,
}

impl From<SimilarityAlg> for CoreSimilarityAlg {
    fn from(mysim: SimilarityAlg) -> Self {
        match mysim {
            SimilarityAlg::pgv_cosine_similarity => CoreSimilarityAlg::pgv_cosine_similarity,
        }
    }
}

#[allow(non_camel_case_types)]
#[derive(Clone, Debug, Serialize, Deserialize, PostgresEnum)]
pub enum IndexDist {
    pgv_hnsw_l2,
    pgv_hnsw_ip,
    pgv_hnsw_cosine,
    vsc_diskann_cosine,
}

impl From<IndexDist> for CoreIndexDist {
    fn from(myindexdist: IndexDist) -> Self {
        match myindexdist {
            IndexDist::pgv_hnsw_l2 => CoreIndexDist::pgv_hnsw_l2,
            IndexDist::pgv_hnsw_ip => CoreIndexDist::pgv_hnsw_ip,
            IndexDist::pgv_hnsw_cosine => CoreIndexDist::pgv_hnsw_cosine,
            IndexDist::vsc_diskann_cosine => CoreIndexDist::vsc_diskann_cosine,
        }
    }
}
