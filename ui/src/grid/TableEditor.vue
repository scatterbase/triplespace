<template>
	<form class="ts-sheet-table" @submit.prevent="save">
		<SheetGrid
			:headers="headers"
			:rows="sheetRows"
			:lead-heading="t( 'ts-sheet-entity' )"
			:cell-label="( h, lead ) => t( 'ts-sheet-cell', h, lead )"
			:menu-label="( h ) => h"
			:remove-label="( lead ) => t( 'ts-sheet-remove-row', lead )"
			@edit="edit"
			@paste="paste"
			@fill="fill"
			@remove="remove"
		/>
		<div class="ts-sheet__panel">
			<label>
				{{ t( 'ts-sheet-add-ids' ) }}
				<input
					v-model="ids"
					class="cdx-text-input__input"
					type="text"
					:placeholder="t( 'ts-sheet-add-ids-placeholder' )"
					@keydown.enter.prevent="addIds">
			</label>
			<CdxButton
				type="button"
				:disabled="saving"
				@click="addIds">
				{{ t( 'ts-sheet-add' ) }}
			</CdxButton>
			<CdxButton
				type="button"
				:disabled="saving"
				@click="addNew">
				{{ t( 'ts-sheet-new-row' ) }}
			</CdxButton>
		</div>
		<div class="ts-sheet__panel">
			<label>
				{{ t( 'ts-sheet-summary' ) }}
				<input
					v-model="summary"
					class="cdx-text-input__input"
					type="text">
			</label>
			<label v-if="reference" class="ts-sheet__filter">
				<input v-model="withReference" type="checkbox">
				{{ t( 'ts-sheet-with-reference' ) }}
			</label>
		</div>
		<CdxMessage v-if="problem" type="error">
			{{ problem }}
		</CdxMessage>
		<div class="ts-sheet__actions">
			<CdxButton
				type="submit"
				action="progressive"
				weight="primary"
				:disabled="saving || !dirty">
				{{ saving ? t( 'ts-sheet-saving' ) : t( 'ts-sheet-save' ) }}
			</CdxButton>
			<CdxButton
				type="button"
				:disabled="saving"
				@click="cancel">
				{{ anySaved ? t( 'ts-sheet-done' ) : t( 'ts-sheet-cancel' ) }}
			</CdxButton>
		</div>
	</form>
</template>

<script setup lang="ts">
// A Table page's grid, edited (ADR 0045 §6, §7; 15 §2.5, §2.6). Edits gather by row; saving
// writes each changed row as one change on its entity, with the row's own base revision,
// a summary naming the table and the tag `table:{page id}`, in order, each succeeding or
// failing alone. Rows added and removed are one edit of the page's definition, made after
// the entities are written. A new row creates an item from its cells, then joins the
// table; the two writes are not one, and a failure of the second is said.
import { computed, ref } from 'vue';
import { CdxButton, CdxMessage } from '@wikimedia/codex';
import { ApiError } from '../edit/api';
import type { Api } from '../edit/api';
import SheetGrid from './SheetGrid.vue';
import type { SheetHeader, SheetRow } from './sheet';
import { msg } from './data';
import type { TableData, TableRow } from './data';
import { defaultReference, definitionEdit, isProblem, rowEdit } from './tableedit';
import type { Claim } from './tableedit';

const props = defineProps<{
	data: TableData;
	api: Api;
}>();

const emit = defineEmits<{
	saved: [];
	cancel: [];
}>();

const t = ( key: string, ...params: string[] ): string => msg( props.data, key, ...params );

/** A row being edited: one the table lists, one being added by ID, or a new item. */
interface Editing {
	key: string;
	/** The row as it was read; null for a row not in the table yet. */
	before: TableRow | null;
	/** The entity's ID; empty for an item not created yet. */
	id: string;
	cells: string[];
	/** What saving said of it. */
	outcome: string;
	failed: boolean;
	saved: boolean;
}

const rows = ref<Editing[]>( props.data.rows.map( ( r ) => ( {
	key: r.id,
	before: r,
	id: r.id,
	cells: r.cells.map( ( c ) => c.text ),
	outcome: '',
	failed: false,
	saved: false
} ) ) );
const removed = ref<string[]>( [] );
const ids = ref( '' );
const summary = ref( '' );
const saving = ref( false );
const problem = ref( '' );
const stored = JSON.parse( props.data.definition ) as Record<string, unknown>;
const reference = defaultReference( stored );
const withReference = ref( reference !== null );
let fresh = 0;

const headers = computed<SheetHeader[]>(
	() => props.data.columns.map( ( c ) => ( { label: c.label, sub: c.sub } ) )
);

const changedCell = ( r: Editing, c: number ): boolean => ( r.before ?
	r.before.cells[ c ].editable && r.before.cells[ c ].text !== r.cells[ c ].trim() :
	r.cells[ c ].trim() !== '' );

const sheetRows = computed<SheetRow[]>( () => rows.value.map( ( r ) => ( {
	key: r.key,
	lead: r.id || t( 'ts-sheet-new-item' ),
	href: r.before?.href,
	status: r.outcome || ( r.before?.missing ? t( 'ts-sheet-missing' ) : undefined ),
	kind: r.failed ? 'invalid' : ( r.saved ? 'matches' : undefined ),
	removable: true,
	cells: r.cells,
	// A row added by its ID shows nothing until the page is drawn again.
	readonly: props.data.columns.map( ( col, c ) => ( r.before ?
		!r.before.cells[ c ].editable :
		r.id !== '' || col.kind === 'other' ) ),
	changed: r.cells.map( ( _, c ) => changedCell( r, c ) ),
	notes: r.before ? r.before.cells.map( ( c, i ) => ( changedCell( r, i ) ? '' : c.note ) ) : undefined
} ) ) );

const changedAny = ( r: Editing ): boolean => r.cells.some( ( _, c ) => changedCell( r, c ) );
const dirty = computed( () => removed.value.length > 0 ||
	rows.value.some( ( r ) => !r.saved && ( !r.before || changedAny( r ) ) ) );
const anySaved = computed( () => rows.value.some( ( r ) => r.saved ) );

const at = ( key: string ): Editing | undefined => rows.value.find( ( r ) => r.key === key );

function edit( key: string, c: number, value: string ): void {
	const r = at( key );
	if ( r ) {
		r.cells[ c ] = value;
		r.saved = false;
	}
}

function paste( key: string, c: number, cells: string[][] ): void {
	const start = rows.value.findIndex( ( r ) => r.key === key );
	const readonly = sheetRows.value.map( ( r ) => r.readonly ?? [] );
	cells.forEach( ( line, dr ) => {
		const r = rows.value[ start + dr ];
		if ( !r ) {
			return;
		}
		line.forEach( ( v, dc ) => {
			if ( c + dc < r.cells.length && !readonly[ start + dr ][ c + dc ] ) {
				r.cells[ c + dc ] = v.trim();
				r.saved = false;
			}
		} );
	} );
}

function fill( key: string, c: number ): void {
	const i = rows.value.findIndex( ( r ) => r.key === key );
	if ( i > 0 ) {
		rows.value[ i ].cells[ c ] = rows.value[ i - 1 ].cells[ c ];
		rows.value[ i ].saved = false;
	}
}

// Removing a row takes it out of the table and never touches its entity.
function remove( key: string ): void {
	const r = at( key );
	if ( !r ) {
		return;
	}
	if ( r.before ) {
		removed.value.push( r.id );
	}
	rows.value = rows.value.filter( ( x ) => x.key !== key );
}

function blank(): string[] {
	return props.data.columns.map( () => '' );
}

function addIds(): void {
	problem.value = '';
	const wanted = ids.value.split( /[\s,|]+/ ).map( ( x ) => x.trim() ).filter( ( x ) => x !== '' );
	// A letter or a provider code and a number, or a keyed ID; saving the page checks the rest.
	const bad = wanted.find( ( x ) => !/^[A-Za-z]{1,4}\d/.test( x ) && !x.includes( ':' ) );
	if ( bad ) {
		problem.value = t( 'ts-sheet-not-id', bad );
		return;
	}
	for ( const id of wanted ) {
		const upper = /:/.test( id ) ? id : id.toUpperCase();
		if ( !rows.value.some( ( r ) => r.id.toUpperCase() === upper.toUpperCase() ) ) {
			rows.value.push( { key: `add-${ upper }`, before: null, id: upper, cells: blank(), outcome: '', failed: false, saved: false } );
		}
		removed.value = removed.value.filter( ( x ) => x.toUpperCase() !== upper.toUpperCase() );
	}
	ids.value = '';
}

function addNew(): void {
	fresh++;
	rows.value.push( { key: `new-${ fresh }`, before: null, id: '', cells: blank(), outcome: '', failed: false, saved: false } );
}

function cancel(): void {
	if ( anySaved.value ) {
		emit( 'saved' );
	} else {
		emit( 'cancel' );
	}
}

function said( e: unknown ): string {
	if ( e instanceof ApiError && e.code === 'editconflict' ) {
		return t( 'ts-sheet-conflict' );
	}
	return e instanceof Error ? e.message : String( e );
}

async function claimsOf( id: string ): Promise<Record<string, Claim[]>> {
	const body = await props.api.get( { action: 'wbgetentities', ids: id, props: 'claims' } );
	const entity = ( body.entities as Record<string, { claims?: Record<string, Claim[]> }> )[ id ];
	return entity?.claims ?? {};
}

async function save(): Promise<void> {
	saving.value = true;
	problem.value = '';
	const tags = `table:${ props.data.pageId }`;
	const via = t( 'ts-sheet-via', props.data.title );
	const note = summary.value.trim() === '' ? via : `${ via }: ${ summary.value.trim() }`;
	const carried = withReference.value ? reference : null;
	const added: string[] = [];
	let failed = false;
	for ( const r of rows.value ) {
		if ( r.saved ) {
			continue;
		}
		r.outcome = '';
		r.failed = false;
		try {
			if ( !r.before && r.id !== '' ) {
				// A row added by its ID: nothing to write on the entity.
				added.push( r.id );
				continue;
			}
			const touchesStatement = r.before !== null &&
				r.before.cells.some( ( c, i ) => c.guid && changedCell( r, i ) );
			const data = rowEdit(
				props.data.columns,
				r.before,
				r.cells,
				touchesStatement ? await claimsOf( r.id ) : {},
				carried
			);
			if ( isProblem( data ) ) {
				throw new Error( t( 'ts-sheet-bad-value', data.text, props.data.columns[ data.column ].label ) );
			}
			if ( data === null ) {
				continue;
			}
			if ( r.before ) {
				await props.api.write( {
					action: 'wbeditentity',
					id: r.id,
					baserevid: r.before.lastrevid || undefined,
					data: JSON.stringify( data ),
					summary: note,
					tags
				} );
			} else {
				const body = await props.api.write( {
					action: 'wbeditentity',
					new: 'item',
					data: JSON.stringify( data ),
					summary: note,
					tags
				} );
				r.id = String( ( body.entity as { id: string } ).id );
				added.push( r.id );
			}
			r.saved = true;
			r.outcome = t( 'ts-sheet-saved' );
		} catch ( e ) {
			failed = true;
			r.failed = true;
			r.outcome = said( e );
		}
	}
	// The rows are part of the definition: one edit of the page for all of them.
	try {
		const text = definitionEdit( props.data.definition, added, removed.value );
		if ( text !== null ) {
			await props.api.write( {
				action: 'edit',
				title: props.data.title,
				text,
				baserevid: props.data.revid,
				summary: note
			} );
		}
	} catch ( e ) {
		failed = true;
		problem.value = t( 'ts-sheet-rows-failed', added.join( ', ' ) || '–', said( e ) );
	}
	saving.value = false;
	if ( !failed ) {
		emit( 'saved' );
	}
}
</script>
