//! lab-storage — SQLite 用户态数据(学习进度 + 习题记录)。
//! 单用户单库;rusqlite 同步 API,由调用方在 blocking 上下文中使用。

use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};

pub struct Storage {
    // Connection 非 Sync;Mutex 使 Storage 可放进共享状态
    conn: std::sync::Mutex<Connection>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LessonProgress {
    pub lesson_id: String,
    pub status: String, // not_started | in_progress | done
    pub updated_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExerciseAttempt {
    pub id: i64,
    pub exercise_id: String,
    pub attempt_no: i64,
    pub status: String,    // wrong | passed | revealed
    pub answer_json: String,
    pub judged_json: String,
    pub created_at: i64,
}

impl Storage {
    pub fn open(path: &std::path::Path) -> Result<Self, String> {
        let conn = Connection::open(path).map_err(|e| e.to_string())?;
        conn.execute_batch(
            "PRAGMA journal_mode=WAL;
             CREATE TABLE IF NOT EXISTS lesson_progress (
                lesson_id TEXT PRIMARY KEY,
                status TEXT NOT NULL DEFAULT 'not_started',
                updated_at INTEGER NOT NULL DEFAULT 0
             );
             CREATE TABLE IF NOT EXISTS exercise_attempt (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                exercise_id TEXT NOT NULL,
                attempt_no INTEGER NOT NULL,
                status TEXT NOT NULL,
                answer_json TEXT NOT NULL,
                judged_json TEXT NOT NULL,
                created_at INTEGER NOT NULL
             );
             CREATE INDEX IF NOT EXISTS idx_attempt_ex ON exercise_attempt(exercise_id);",
        )
        .map_err(|e| e.to_string())?;
        Ok(Self { conn: std::sync::Mutex::new(conn) })
    }

    pub fn get_progress(&self, lesson_id: &str) -> Option<LessonProgress> {
        let conn = self.conn.lock().unwrap();
        let conn = &*conn;
        conn
            .query_row(
                "SELECT lesson_id, status, updated_at FROM lesson_progress WHERE lesson_id = ?1",
                params![lesson_id],
                |r| {
                    Ok(LessonProgress {
                        lesson_id: r.get(0)?,
                        status: r.get(1)?,
                        updated_at: r.get(2)?,
                    })
                },
            )
            .ok()
    }

    pub fn set_progress(&self, lesson_id: &str, status: &str) -> Result<(), String> {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);
        let conn = self.conn.lock().unwrap();
        conn
            .execute(
                "INSERT INTO lesson_progress (lesson_id, status, updated_at) VALUES (?1, ?2, ?3)
                 ON CONFLICT(lesson_id) DO UPDATE SET status = ?2, updated_at = ?3",
                params![lesson_id, status, now],
            )
            .map_err(|e| e.to_string())?;
        Ok(())
    }

    /// 记录一次作答,返回本次的 attempt 序号(用于提示梯度)。
    pub fn record_attempt(
        &self,
        exercise_id: &str,
        status: &str,
        answer_json: &str,
        judged_json: &str,
    ) -> Result<i64, String> {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);
        let conn = self.conn.lock().unwrap();
        let prev: i64 = conn
            .query_row(
                "SELECT COALESCE(MAX(attempt_no), 0) FROM exercise_attempt WHERE exercise_id = ?1",
                params![exercise_id],
                |r| r.get(0),
            )
            .map_err(|e| e.to_string())?;
        let attempt_no = prev + 1;
        conn
            .execute(
                "INSERT INTO exercise_attempt (exercise_id, attempt_no, status, answer_json, judged_json, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![exercise_id, attempt_no, status, answer_json, judged_json, now],
            )
            .map_err(|e| e.to_string())?;
        Ok(attempt_no)
    }

    /// 某题的历史作答(判题反馈与提示梯度用)
    /// 每道题的最近一次作答(错题本/掌握度用)
    pub fn latest_attempts(&self) -> Vec<ExerciseAttempt> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = match conn
            .prepare(
                "SELECT a.id, a.exercise_id, a.attempt_no, a.status, a.answer_json, a.judged_json, a.created_at                  FROM exercise_attempt a                  JOIN (SELECT exercise_id, MAX(id) AS mid FROM exercise_attempt GROUP BY exercise_id) t                  ON a.id = t.mid ORDER BY a.id DESC",
            )
        {
            Ok(s) => s,
            Err(_) => return Vec::new(),
        };
        let rows = stmt.query_map([], |r| {
            Ok(ExerciseAttempt {
                id: r.get(0)?,
                exercise_id: r.get(1)?,
                attempt_no: r.get(2)?,
                status: r.get(3)?,
                answer_json: r.get(4)?,
                judged_json: r.get(5)?,
                created_at: r.get(6)?,
            })
        });
        match rows {
            Ok(iter) => iter.flatten().collect(),
            Err(_) => Vec::new(),
        }
    }

    pub fn attempts(&self, exercise_id: &str) -> Vec<ExerciseAttempt> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = match conn
            .prepare("SELECT id, exercise_id, attempt_no, status, answer_json, judged_json, created_at FROM exercise_attempt WHERE exercise_id = ?1 ORDER BY attempt_no")
        {
            Ok(s) => s,
            Err(_) => return Vec::new(),
        };
        let rows = stmt.query_map(params![exercise_id], |r| {
            Ok(ExerciseAttempt {
                id: r.get(0)?,
                exercise_id: r.get(1)?,
                attempt_no: r.get(2)?,
                status: r.get(3)?,
                answer_json: r.get(4)?,
                judged_json: r.get(5)?,
                created_at: r.get(6)?,
            })
        });
        match rows {
            Ok(iter) => iter.flatten().collect(),
            Err(_) => Vec::new(),
        }
    }
}
