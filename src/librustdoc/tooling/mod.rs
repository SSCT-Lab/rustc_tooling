use rustc_data_structures::fx::FxHashMap;

use rustc_middle::ty::TyCtxt;
use rustc_middle::hir::map::Map;
use rustc_session::Session;
use rustc_span::source_map::SourceMap;
use rustc_hir::intravisit::Visitor;
use rustc_hir::{Expr, ExprKind, HirId, QPath};
use rustc_span::FileName;
use rustc_hir::Node;

#[derive(Debug, Eq, Hash, PartialEq, Clone)] 
#[allow(dead_code)]
pub(crate) struct LocInfo {
    pub ident: String,
    pub line_num: usize,
    pub col_num: usize,
    pub file_path: FileName,
}

#[allow(dead_code)]
pub(crate) struct DependencyGraph<'tcx> {
    tcx: TyCtxt<'tcx>,
    hir: Map<'tcx>,
    lhs_to_loc_info: FxHashMap<LocInfo, Vec<LocInfo>>,
}

#[allow(dead_code)]
impl<'tcx> DependencyGraph<'tcx> {
    fn sess(&self) -> &'tcx Session {
        self.tcx.sess
    }

    fn source_map(&self) -> &SourceMap {
        self.sess().source_map()
    }
}

#[allow(dead_code)]
pub struct GraphVisitor<'tcx> {
    graph: DependencyGraph<'tcx>,
    current_body_id: Option<rustc_hir::BodyId>,
}

#[allow(dead_code)]
impl<'tcx> GraphVisitor<'tcx> {
    fn new(graph: DependencyGraph<'tcx>) -> Self {
        GraphVisitor {
            graph,
            current_body_id: None,
        }
    }

    fn update_current_body_id(&mut self, body_id: Option<rustc_hir::BodyId>) {
        self.current_body_id = body_id;
    }
}

#[allow(dead_code)]
impl GraphVisitor<'_> {
    // extract for lhs in assign expr
    fn extract_loc_info(&self, expr: &Expr<'_>) -> Option<LocInfo> {
        if let ExprKind::Path(qpath) = &expr.kind {
            if let QPath::Resolved(_, path) = qpath {
                if let Some(segment) = path.segments.last() {
                    let ident = segment.ident.to_string();
                    let src_map = self.graph.source_map();
                    let span = segment.ident.span;
        
                    let file_path = src_map.span_to_filename(span);
                    let start_pos = src_map.lookup_char_pos(span.lo());
        
                    return Some(LocInfo {
                        ident,
                        line_num: start_pos.line,
                        col_num: start_pos.col_display,
                        file_path,
                    });
                }
            }
        }

        None
    }

    // extract info for rhs(s)
    fn extract_loc_infos(&self, expr: &Expr<'_>) -> Option<Vec<LocInfo>> {    
        match expr.kind {
            ExprKind::Binary(_, lhs, rhs) => {
                let mut loc_infos = Vec::new();
    
                if let Some(lhs_loc_infos) = self.extract_loc_infos(lhs) {
                    loc_infos.extend(lhs_loc_infos);
                }
    
                if let Some(rhs_loc_infos) = self.extract_loc_infos(rhs) {
                    loc_infos.extend(rhs_loc_infos);
                }
    
                Some(loc_infos)
            },
            ExprKind::Call(expr, _) => {
                if let Some((hir_id, ident)) = self.get_hir_id_and_ident(expr) {
                    if let Some(loc_info) = self.extract_loc_info_from_hir_id(hir_id, ident) {
                        Some(vec![loc_info])
                    } else {
                        None
                    }
                } else {
                    None
                }
            },
            ExprKind::MethodCall(method_name, _, _, _) => {
                match self.current_body_id {
                    Some(body) => {
                        let typeck_results = self.graph.tcx.typeck_body(body);
                        let def_id = typeck_results.type_dependent_def(expr.hir_id);
                        match def_id {
                            Some((_, def_id)) => {
                                if let Ok(span) = self.graph.tcx.span_of_impl(def_id) {
                                    let src_map = self.graph.source_map();
                                    let file_path = src_map.span_to_filename(span);
                                    let start_pos = src_map.lookup_char_pos(span.lo());
                                    
                                    return Some(vec![LocInfo {
                                        ident: method_name.ident.to_string(),
                                        line_num: start_pos.line,
                                        col_num: start_pos.col_display,
                                        file_path,
                                    }]);
                                }               
                            },
                            None => return None
                        }
                    },
                    None => return None
                }
                None
            },
            ExprKind::Path(_) => {
                if let Some((hir_id, ident)) = self.get_hir_id_and_ident(expr) {
                    if let Some(loc_info) = self.extract_loc_info_from_hir_id(hir_id, ident) {
                        Some(vec![loc_info])
                    } else {
                        None
                    }
                } else {
                    None
                }
            },
            _ => None,
        }
    }
    
    fn get_hir_id_and_ident(&self, expr: &Expr<'_>) -> Option<(HirId, String)> {
        println!("ExprKind: {:?}", expr.kind);
        if let ExprKind::Path(qpath) = &expr.kind {
            if let QPath::Resolved(_, path) = qpath {
                if let Some(segment) = path.segments.last() {
                    let ident = segment.ident.to_string();
                    println!("Ident: {}", ident);
                    if let Some(def_id) = path.res.opt_def_id() {
                        let Some(node) = self.graph.tcx.hir().get_if_local(def_id) else {
                            return None;
                        };
                        if let Node::Item(item) = node {
                            return Some((item.hir_id(), ident));
                        }
                    } else {
                        return Some((expr.hir_id, ident));
                    }
                }
            }
        }
        None
    }

    fn extract_loc_info_from_hir_id(&self, hir_id: HirId, ident: String) -> Option<LocInfo> {
        use crate::rustc_hir::intravisit::Map;
        
        let hir = self.graph.hir;
        
        let node = match hir.find(hir_id) {
            Some(node) => node,
            None => return None
        };

        let src_map = self.graph.source_map();
        let span = match node {                
            Node::Expr(expr) => expr.span,
            Node::Item(item) => item.span,
            _ => return None, 
        };
        let file_path = src_map.span_to_filename(span);
        let start_pos = src_map.lookup_char_pos(span.lo());

        Some(
            LocInfo {
                ident,
                line_num: start_pos.line,
                col_num: start_pos.col_display,
                file_path,
            }
        )
    }

    fn extract_ident_from_pat(&self, pat: rustc_hir::Pat<'_>) -> Option<String> {
        use rustc_hir::PatKind::*;
        match pat.kind {
            Binding(_, _, ident, None) => Some(ident.to_string()),
            _ => None,
        }
    }
}

impl<'tcx> Visitor<'tcx> for GraphVisitor<'tcx> {
    fn visit_item(&mut self, item: &'tcx rustc_hir::Item<'tcx>) {
        rustc_hir::intravisit::walk_item(self, item);
        if let rustc_hir::ItemKind::Fn(.., body_id) = &item.kind {
            self.update_current_body_id(Some(*body_id));
            let body = self.graph.tcx.hir().body(*body_id);
            self.visit_body(body);
        }
    }

    fn visit_stmt(&mut self, stmt: &'tcx rustc_hir::Stmt<'tcx>) {
        use rustc_hir::StmtKind::*;

        match stmt.kind {
            Local(local) => {
                if let Some(ident) = self.extract_ident_from_pat(*local.pat) {
                    if let Some(init_expr) = local.init {
                        if let Some(rhs_loc_infos) = self.extract_loc_infos(init_expr) {
                            let span = local.span;
                            let src_map = self.graph.source_map();
                            let start_pos = src_map.lookup_char_pos(span.lo());
                            let file_path = src_map.span_to_filename(span);

                            let lhs_loc_info = LocInfo {
                                ident,
                                line_num: start_pos.line,
                                col_num: start_pos.col_display,
                                file_path,
                            };

                            self.graph.lhs_to_loc_info.entry(lhs_loc_info)
                                .or_insert(Vec::new())
                                .extend(rhs_loc_infos)
                        }
                    }
                }
            }
            _ => {}
        }

        rustc_hir::intravisit::walk_stmt(self, stmt);
    }
    

    fn visit_expr(&mut self, ex: &'tcx Expr<'tcx>) {
        if let ExprKind::Assign(lhs, rhs, _) = &ex.kind {
            // Extract location information for the lhs of the assignment
            if let Some(lhs_loc_info) = self.extract_loc_info(lhs) {
                // Initialize a vector to hold LocInfo objects for all expressions contributing to the rhs value
                let mut rhs_loc_infos = Vec::new();

                // Recursively visit rhs to extract location information for all contributing expressions
                if let Some(rhs_info) = self.extract_loc_infos(rhs) {
                    rhs_loc_infos.extend(rhs_info);
                }

                // Update the lhs_to_loc_info map in the DependencyGraph
                // If there's already an entry for this lhs, append to it; otherwise, create a new entry
                self.graph.lhs_to_loc_info.entry(lhs_loc_info)
                    .and_modify(|e| e.extend(rhs_loc_infos.clone()))
                    .or_insert(rhs_loc_infos);
            }
        }
        rustc_hir::intravisit::walk_expr(self, ex);
    }
}

#[allow(dead_code)]
pub fn analyze_dependencies(tcx: TyCtxt<'_>) {
    let hir = tcx.hir();
    let dependency_graph = DependencyGraph {
        tcx,
        hir,
        lhs_to_loc_info: FxHashMap::default(), // Initialize the map
    };

    let mut visitor = GraphVisitor::new(dependency_graph);

    // Visit all item likes in the crate
    tcx.hir().visit_all_item_likes_in_crate(&mut visitor);

    println!("Dependency Graph:");
    for (lhs, rhs_vec) in &visitor.graph.lhs_to_loc_info {
        println!("LHS: {:?}", lhs);
        for rhs in rhs_vec {
            println!("\tRHS: {:?}", rhs);
        }
    }
}
