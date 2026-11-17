//! 表格结构操作：行列增删与源码重建。

use super::*;

/// 由单元格内容还原出 Markdown 表格源码。
/// 表格工具栏的操作。行/列下标按「含表头」计：0 = 表头行，数据行从 1 起。
#[derive(Clone, Copy, Debug)]
pub enum TableOp {
    /// 在指定行下方插入空行（指定表头行 0 时插到数据区最前）
    InsertRow(usize),
    /// 删除指定数据行（表头行 0 不可删）
    DeleteRow(usize),
    /// 在指定列右侧插入空列
    InsertCol(usize),
    /// 删除指定列（至少保留一列）
    DeleteCol(usize),
    /// 整表所有列设置为同一种对齐
    SetAlign(crate::ast::Align),
    /// 删除整张表格（变成空段落）
    Delete,
}

/// 对字符串网格执行表格结构操作，返回是否发生变化。
/// 行/列下标按「含表头」计：0 = 表头行，数据行从 1 起（grid[0] 即表头）。
pub(crate) fn table_op_on_grid(
    grid: &mut Vec<Vec<String>>,
    align: &mut Vec<crate::ast::Align>,
    op: TableOp,
) -> bool {
    use crate::ast::Align;
    let cols = grid
        .first()
        .map(|r| r.len())
        .into_iter()
        .fold(0usize, usize::max);
    match op {
        TableOp::InsertRow(r) => {
            // 在第 r 行下方插入：r=0（表头）→ 数据区最前；其余插到其后
            grid.insert((r + 1).min(grid.len()), vec![String::new(); cols.max(1)]);
            true
        }
        TableOp::DeleteRow(r) => {
            // 表头行不可删
            if r >= 1 && grid.len() > 1 {
                grid.remove(r.min(grid.len() - 1));
                true
            } else {
                false
            }
        }
        TableOp::InsertCol(c) => {
            let at = (c + 1).min(cols.max(1));
            for row in grid.iter_mut() {
                row.insert(at.min(row.len()), String::new());
            }
            align.insert(at.min(align.len()), Align::Left);
            true
        }
        TableOp::DeleteCol(c) => {
            if cols > 1 {
                let at = c.min(cols - 1);
                for row in grid.iter_mut() {
                    if at < row.len() {
                        row.remove(at);
                    }
                }
                if at < align.len() {
                    align.remove(at);
                }
                true
            } else {
                false
            }
        }
        TableOp::SetAlign(a) => {
            for al in align.iter_mut() {
                *al = a;
            }
            true
        }
        TableOp::Delete => false,
    }
}

/// 对第 idx 块（表格）执行结构操作：解析块、改结构、重建源码写回。
pub fn apply_table_op(doc: &mut Document, idx: usize, op: TableOp) {
    use crate::ast::Block;
    let Some(Block::Table {
        head, rows, align, ..
    }) = doc.blocks.get(idx)
    else {
        return;
    };
    let head = head.clone();
    let rows = rows.clone();
    let mut align = align.clone();
    if let TableOp::Delete = op {
        doc.record();
        doc.edit_block(idx, "");
        return;
    }
    let mut grid: Vec<Vec<String>> =
        vec![head.iter().map(|c| crate::ast::inline_plain(c)).collect()];
    grid.extend(rows.iter().map(|r| {
        r.iter()
            .map(|c| crate::ast::inline_plain(c))
            .collect::<Vec<String>>()
    }));
    if !table_op_on_grid(&mut grid, &mut align, op) {
        return;
    }
    doc.record();
    doc.edit_block(idx, &table_source(&grid, &align));
}

pub(crate) fn table_source(grid: &[Vec<String>], align: &[crate::ast::Align]) -> String {
    use crate::ast::Align;
    let cols = grid.iter().map(|r| r.len()).max().unwrap_or(0);
    let mut s = String::new();
    let row = |cells: &Vec<String>| {
        let mut line = String::from("|");
        for c in 0..cols {
            line.push(' ');
            line.push_str(cells.get(c).map(|x| x.as_str()).unwrap_or(""));
            line.push_str(" |");
        }
        line
    };
    if let Some(head) = grid.first() {
        s.push_str(&row(head));
        s.push('\n');
    }
    s.push('|');
    for c in 0..cols {
        s.push_str(match align.get(c).copied().unwrap_or(Align::Left) {
            Align::Left => " :--- |",
            Align::Center => " :---: |",
            Align::Right => " ---: |",
        });
    }
    for r in grid.iter().skip(1) {
        s.push('\n');
        s.push_str(&row(r));
    }
    s
}
