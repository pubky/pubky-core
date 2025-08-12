use sea_query::{ColumnDef, Expr, Query, SimpleExpr, Table};
use sqlx::{Row, Transaction};

use crate::persistence::sql::{db_connection::DbConnection, migration::MigrationTrait, migrations::M20250806CreateUserMigration};

/// The name of the migration table to keep track of which migrations have been applied.
const MIGRATION_TABLE: &str = "migrations";

/// Migrator is responsible for running migrations on the database.
pub struct Migrator<'a> {
    db: &'a DbConnection,
}

impl<'a> Migrator<'a> {
    /// Creates a new migrator.
    /// db: The database connection to use.
    pub fn new(db: &'a DbConnection) -> Self {
        Self { db }
    }

    /// Returns a list of migrations to run.
    fn migrations() -> Vec<Box<dyn MigrationTrait>> {
        // Add new migrations here. They run from top to bottom.
        vec![
            Box::new(M20250806CreateUserMigration)
        ]
    }

    /// Runs all migrations that are not yet applied.
    pub async fn run(&self) -> anyhow::Result<()> {
        self.run_migrations(Self::migrations()).await
    }

    /// Runs a specific list of migrations.
    pub async fn run_migrations(&self, migrations: Vec<Box<dyn MigrationTrait>>) -> anyhow::Result<()> {
        self.create_migration_table().await?;
        let already_applied_migrations = self.get_applied_migrations().await?;
        let migrations_to_run = migrations
            .into_iter()
            .filter(|m| !already_applied_migrations.contains(&m.name().to_string()))
            .collect::<Vec<_>>();

        for migration in migrations_to_run {
            self.run_migration(&*migration).await?;
        }
        Ok(())
    }

    /// Runs a single migration.
    async fn run_migration(&self, migration: &dyn MigrationTrait) -> anyhow::Result<()> {
        tracing::info!("Running migration {}", migration.name());
        let mut tx = self.db.pool().begin().await?;
        // Execute the migration
        let result: Result<(), anyhow::Error> = {
            migration.up(self.db, &mut tx).await?;
            self.mark_migration_as_done(&mut tx, migration.name())
                .await?;
            Ok(())
        };

        // Depending on the result, commit or rollback the transaction
        match result {
            Ok(_) => {
                tx.commit().await?;
                tracing::info!("Migration {} applied successfully", migration.name());
            }
            Err(e) => {
                tracing::error!("Failed to run migration {}: {}", migration.name(), e);
                tx.rollback().await?;
            }
        }
        Ok(())
    }

    /// Creates the migration table if it doesn't exist.
    /// This table keeps track of which migrations have been applied.
    async fn create_migration_table(&self) -> anyhow::Result<()> {
        let statement = Table::create()
            .table(MIGRATION_TABLE)
            .if_not_exists()
            .col(
                ColumnDef::new("id")
                    .integer()
                    .primary_key()
                    .auto_increment()
                    .not_null(),
            )
            .col(ColumnDef::new("name").string().not_null().unique_key())
            .col(
                ColumnDef::new("created_at")
                    .timestamp()
                    .not_null()
                    .default(Expr::current_timestamp()),
            )
            .to_owned();
        let query = self.db.build_schema(statement);
        sqlx::query(query.as_str()).execute(self.db.pool()).await?;
        Ok(())
    }

    /// Returns a list of migrations that have already run.
    async fn get_applied_migrations(&self) -> anyhow::Result<Vec<String>> {
        let statement = Query::select()
        .column("name")
        .from(MIGRATION_TABLE).to_owned();
        let (query, _) = self.db.build_query(statement);

        let rows = sqlx::query(&query).fetch_all(self.db.pool()).await?;

        let migration_names: Vec<String> = rows
            .iter()
            .map(|row| row.try_get::<String, _>("name").unwrap_or_default())
            .collect();

        Ok(migration_names)
    }

    /// Marks a migration as done.
    /// This is done by inserting a row into the migrations table with the migration name.
    async fn mark_migration_as_done(
        &self,
        tx: &mut Transaction<'static, sqlx::Postgres>,
        migration_name: &str,
    ) -> anyhow::Result<()> {
        let statement = Query::insert()
        .into_table(MIGRATION_TABLE)
        .columns(["name"])
        .values([SimpleExpr::Value(migration_name.into())])?
        .to_owned();
        let (query, values) = self.db.build_query(statement);

        sqlx::query_with(&query, values).execute(&mut **tx).await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test(flavor = "multi_thread")]
    async fn test_create_table() {
        let db = DbConnection::test_without_migrations().await;
        let migrator = Migrator::new(&db);
        migrator.create_migration_table().await.unwrap();
        let mut tx = db.pool().begin().await.unwrap();
        migrator
            .mark_migration_as_done(&mut tx, "test_migration")
            .await
            .unwrap();
        tx.commit().await.unwrap();
        let applied_migrations = migrator.get_applied_migrations().await.unwrap();
        assert_eq!(applied_migrations, vec!["test_migration"]);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_run_successful() {
        struct TestMigration;
        #[async_trait::async_trait]
        impl MigrationTrait for TestMigration {
            fn name(&self) -> &str {
                "test_migration"
            }

            async fn up(
                &self,
                db: &DbConnection,
                tx: &mut Transaction<'static, sqlx::Postgres>,
            ) -> anyhow::Result<()> {
                let statement = Table::create()
                    .table("test_table")
                    .if_not_exists()
                    .col(
                        ColumnDef::new("id")
                            .integer()
                            .primary_key()
                            .auto_increment()
                            .not_null(),
                    )
                    .to_owned();
                let query = db.build_schema(statement);
                sqlx::query(query.as_str()).execute(&mut **tx).await?;
                Ok(())
            }
        }

        let db = DbConnection::test_without_migrations().await;
        let migrator = Migrator::new(&db);
        migrator
            .run_migrations(vec![Box::new(TestMigration)])
            .await
            .unwrap();
        let applied_migrations = migrator.get_applied_migrations().await.unwrap();
        assert_eq!(applied_migrations, vec!["test_migration"]);

        sqlx::query("SELECT FROM pg_tables WHERE schemaname = 'public' AND tablename='test_table';")
                .fetch_one(db.pool())
                .await
                .expect("Table should exist");


    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_run_failed_rollback() {
        struct TestMigration;
        #[async_trait::async_trait]
        impl MigrationTrait for TestMigration {
            fn name(&self) -> &str {
                "test_migration"
            }

            async fn up(
                &self,
                db: &DbConnection,
                tx: &mut Transaction<'static, sqlx::Postgres>,
            ) -> anyhow::Result<()> {
                // Create table
                let statement = Table::create()
                    .table("test_table")
                    .if_not_exists()
                    .col(
                        ColumnDef::new("id")
                            .integer()
                            .primary_key()
                            .auto_increment()
                            .not_null(),
                    )
                    .to_owned();
                let query = db.build_schema(statement);
                sqlx::query(query.as_str()).execute(&mut **tx).await?;
                // Fail after the table is created
                anyhow::bail!("test error");
            }
        }

        let db = DbConnection::test_without_migrations().await;
        let migrator = Migrator::new(&db);
        migrator
            .run_migrations(vec![Box::new(TestMigration)])
            .await
            .expect_err("Migration should fail");
        let applied_migrations = migrator.get_applied_migrations().await.unwrap();
        assert!(applied_migrations.is_empty());

        let result = sqlx::query("SELECT name FROM sqlite_master WHERE type='table' AND name='test_table';")
            .fetch_one(db.pool())
            .await;
        assert!(result.is_err(), "Table should not exist");
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_run_migration_twice() {
        struct TestMigration;
        #[async_trait::async_trait]
        impl MigrationTrait for TestMigration {
            fn name(&self) -> &str {
                "test_migration"
            }

            async fn up(
                &self,
                _: &DbConnection,
                _: &mut Transaction<'static, sqlx::Postgres>,
            ) -> anyhow::Result<()> {
                Ok(())
            }
        }

        let db = DbConnection::test_without_migrations().await;
        let migrator = Migrator::new(&db);
        migrator.create_migration_table().await.unwrap();
        // Mark the migration as done
        migrator.run_migration(&TestMigration).await.expect("Should work as usual");
        // Try to forcefully run it again
        migrator.run_migration(&TestMigration).await.expect_err("Should fail because it was already run (unique constraint)");
        let applied_migrations = migrator.get_applied_migrations().await.unwrap();
        assert_eq!(applied_migrations.len(), 1, "Migration should only be run once");
    }
}
