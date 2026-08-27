//! hexforge-stream — чистые чанк-примитивы для потокового исполнения
//! (`docs/04-RUST-CORE-ARCHITECTURE.md`, §6).
//!
//! Крейт намеренно не знает о домене: никаких `Transform`, графа или реестра —
//! только арифметика разбиения байтовых диапазонов и аккумуляция выходных
//! чанков. Это сохраняет заявленное правило зависимостей workspace
//! (`hexforge-stream` не зависит от `hexforge-core`) и позволяет тестировать
//! примитивы изолированно.
//!
//! Планировщик, оркестрирующий граф поверх этих примитивов, живёт в
//! `src-tauri/src/scheduler.rs`: он обязан знать домен (реестр, кэш, история),
//! а выносить его в отдельный крейт сейчас означало бы либо цикл по зависимостям,
//! либо дублирование типов. Решение зафиксировано в docs/04 §6 как осознанное
//! отклонение MVP с путём миграции.

/// Размер чанка по умолчанию для chunked-исполнения streamable-операций.
///
/// FR-5.2 требует 64 МБ для файлового чтения в выделенных тредах (NFR-2: constant
/// memory для 32 ГБ файлов). Планировщик использует этот размер для `apply_chunk`
/// и bounded-канала (4 чанка → 256 МБ верхняя граница памяти на стадию).
/// Кооперативная отмена проверяется между чанками — 64 МБ даёт баланс между
/// числом `apply_chunk` вызовов и гранулярностью отмены.
pub const DEFAULT_CHUNK_SIZE_BYTES: usize = 64 * 1024 * 1024;

/// Разбивает диапазон `[0, total_len)` на смежные `(start, end)`-чанки
/// размером до `chunk_size`. Пустой вход не даёт ни одного чанка — итерация
/// `apply_chunk` обязана получить ровно один вызов с пустым срезом и
/// `is_last == true`, что планировщик обрабатывает отдельно.
///
/// Паникует только при `chunk_size == 0` — это ошибка программирования,
/// а не рантайм-условие (деление на ноль всё равно паниковало бы).
pub fn chunk_ranges(total_len: usize, chunk_size: usize) -> Vec<(usize, usize)> {
    assert!(chunk_size > 0, "chunk_size must be non-zero");
    let mut ranges = Vec::new();
    let mut start = 0;
    while start < total_len {
        let end = (start + chunk_size).min(total_len);
        ranges.push((start, end));
        start = end;
    }
    ranges
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_input_yields_no_ranges() {
        assert!(chunk_ranges(0, DEFAULT_CHUNK_SIZE_BYTES).is_empty());
    }

    #[test]
    fn small_input_single_range() {
        assert_eq!(chunk_ranges(5, 16), vec![(0, 5)]);
    }

    #[test]
    fn exact_multiple_no_empty_tail() {
        assert_eq!(chunk_ranges(32, 16), vec![(0, 16), (16, 32)]);
    }

    #[test]
    fn remainder_chunk_is_shorter() {
        assert_eq!(chunk_ranges(35, 16), vec![(0, 16), (16, 32), (32, 35)]);
    }

    #[test]
    fn one_byte_chunks_cover_everything() {
        let ranges = chunk_ranges(4, 1);
        assert_eq!(ranges, vec![(0, 1), (1, 2), (2, 3), (3, 4)]);
    }

    #[test]
    fn ranges_are_contiguous_and_ordered() {
        let total = DEFAULT_CHUNK_SIZE_BYTES * 2 + 12345;
        let ranges = chunk_ranges(total, DEFAULT_CHUNK_SIZE_BYTES);
        let mut expected_start = 0;
        for (i, (start, end)) in ranges.iter().enumerate() {
            assert_eq!(*start, expected_start);
            assert!(end > start);
            let is_last = i == ranges.len() - 1;
            if !is_last {
                assert_eq!(*end - start, DEFAULT_CHUNK_SIZE_BYTES);
            }
            expected_start = *end;
        }
        assert_eq!(expected_start, total);
    }

    #[should_panic(expected = "chunk_size must be non-zero")]
    #[test]
    fn zero_chunk_size_panics() {
        let _ = chunk_ranges(10, 0);
    }
}
