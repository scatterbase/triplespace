<template>
	<div class="ts-sheet-create">
		<MapColumns
			v-if="mapping"
			:data="data"
			:api="api"
			:sheet="mapping"
			@apply="mapped"
			@cancel="mapping = null"
		/>
		<template v-else>
			<div class="ts-sheet__bar">
				<CdxButton
					type="button"
					:aria-pressed="mode === 'grid'"
					@click="show( 'grid' )">
					{{ t( 'ts-sheet-mode-grid' ) }}
				</CdxButton>
				<CdxButton
					type="button"
					:aria-pressed="mode === 'text'"
					@click="show( 'text' )">
					{{ t( 'ts-sheet-mode-text' ) }}
				</CdxButton>
				<template v-if="mode === 'grid'">
					<CdxButton type="button" @click="addRow">
						{{ t( 'ts-sheet-add-row' ) }}
					</CdxButton>
					<CdxButton type="button" @click="panel = panel === 'add' ? null : 'add'">
						{{ t( 'ts-sheet-add-column' ) }}
					</CdxButton>
					<CdxButton
						type="button"
						action="progressive"
						:disabled="checking"
						@click="check">
						{{ checking ? t( 'ts-sheet-checking' ) : t( 'ts-sheet-check' ) }}
					</CdxButton>
					<label class="ts-sheet__filter">
						<input v-model="onlyProblems" type="checkbox">
						{{ t( 'ts-sheet-only-problems' ) }}
					</label>
				</template>
			</div>
			<CdxMessage v-if="notice" type="notice">
				{{ notice }}
			</CdxMessage>
			<CdxMessage v-if="problem" type="error">
				{{ problem }}
			</CdxMessage>
			<template v-if="mode === 'grid'">
				<form
					v-if="panel === 'add'"
					class="ts-sheet__panel"
					@submit.prevent="addColumn">
					<select
						v-model="adding.target"
						class="cdx-select"
						:aria-label="t( 'ts-sheet-add-column' )">
						<option
							v-for="o in addable"
							:key="o"
							:value="o">
							{{ t( `ts-sheet-target-${ o }` ) }}
						</option>
					</select>
					<input
						v-if="isTerm( adding.target )"
						v-model="adding.language"
						class="cdx-text-input__input ts-sheet-map__short"
						type="text"
						:aria-label="t( 'ts-sheet-language' )">
					<EntityLookup
						v-if="adding.target === 'reference'"
						:api="api"
						:lang="data.lang"
						type="property"
						:label="t( 'ts-sheet-reference-of' )"
						:placeholder="t( 'ts-sheet-reference-of' )"
						@pick="( id ) => { adding.of = id; }"
					/>
					<EntityLookup
						v-if="needsProperty( adding.target )"
						:key="adding.target"
						:api="api"
						:lang="data.lang"
						type="property"
						:label="t( 'ts-sheet-property' )"
						:placeholder="t( 'ts-sheet-property' )"
						@pick="( id ) => { adding.property = id; }"
					/>
					<input
						v-if="adding.target === 'new' || adding.target === 'reads'"
						v-model="adding.label"
						class="cdx-text-input__input"
						type="text"
						:aria-label="nameLabel"
						:placeholder="nameLabel">
					<select
						v-if="adding.target === 'new'"
						v-model="adding.datatype"
						class="cdx-select"
						:aria-label="t( 'ts-sheet-datatype' )">
						<option value="">
							{{ t( 'ts-sheet-datatype-later' ) }}
						</option>
						<option
							v-for="d in data.datatypes"
							:key="d"
							:value="d">
							{{ d }}
						</option>
					</select>
					<CdxButton type="submit" action="progressive">
						{{ t( 'ts-sheet-add' ) }}
					</CdxButton>
				</form>
				<form
					v-if="typeof panel === 'number'"
					class="ts-sheet__panel"
					@submit.prevent>
					<strong>{{ headers[ panel ] ? headers[ panel ].label : '' }}</strong>
					<CdxButton type="button" @click="fillDown( panel as number )">
						{{ t( 'ts-sheet-fill-down' ) }}
					</CdxButton>
					<input
						v-model="find"
						class="cdx-text-input__input"
						type="text"
						:aria-label="t( 'ts-sheet-find' )"
						:placeholder="t( 'ts-sheet-find' )">
					<input
						v-model="replacement"
						class="cdx-text-input__input"
						type="text"
						:aria-label="t( 'ts-sheet-replace-with' )"
						:placeholder="t( 'ts-sheet-replace-with' )">
					<CdxButton
						type="button"
						:disabled="find === ''"
						@click="replaceIn( panel as number )">
						{{ t( 'ts-sheet-replace' ) }}
					</CdxButton>
					<CdxButton
						type="button"
						action="destructive"
						@click="removeColumn( panel as number )">
						{{ t( 'ts-sheet-remove-column' ) }}
					</CdxButton>
				</form>
				<CellLookup
					v-if="lookupFor"
					:api="api"
					:lang="data.lang"
					:label="lookupFor.label"
					:at="`${ lookupFor.r }-${ lookupFor.c }`"
					@pick="picked"
				/>
				<SheetGrid
					:headers="headers"
					:rows="shown"
					:lead-heading="t( 'ts-sheet-row' )"
					:status-heading="t( 'ts-sheet-status' )"
					:cell-label="( h, lead ) => t( 'ts-sheet-cell', h, lead )"
					:menu-label="( h ) => t( 'ts-sheet-column-menu', h )"
					:remove-label="( lead ) => t( 'ts-sheet-remove-row', lead )"
					@edit="edit"
					@paste="paste"
					@fill="fillFromAbove"
					@menu="( c ) => { panel = panel === c ? null : c; }"
					@remove="removeRow"
					@focus="focused"
				/>
				<p v-if="outcome" class="ts-sheet__summary">
					{{ outcome }}
				</p>
				<p v-else-if="stale && result" class="ts-sheet__summary ts-sheet__summary--stale">
					{{ t( 'ts-sheet-stale' ) }}
				</p>
				<ul v-if="result && !stale && result.notices.length" class="ts-sheet__notices">
					<li
						v-for="( n, i ) in result.notices"
						:key="i"
						:class="{ 'ts-sheet__notice--error': n.error }">
						{{ n.text }}
					</li>
				</ul>
			</template>
		</template>
	</div>
</template>

<script setup lang="ts">
// The table of Special:CreateManyEntities as a grid, one row per item, over the same
// text the page posts (ADR 0106 §1, §6; 21 §6.7). Every change is written into the
// form's text box, so checking, saving a draft and importing are the form's own, with
// or without this script. The page's check is asked for as JSON to show each row's
// status; what the check writes back into the text (a matched row's `id`, a proposed
// data type) is taken up. A file chosen in the form is mapped here first (0106 §7).
import { computed, onBeforeUnmount, onMounted, reactive, ref } from 'vue';
import { CdxButton, CdxMessage } from '@wikimedia/codex';
import EntityLookup from '../edit/EntityLookup.vue';
import type { Api } from '../edit/api';
import CellLookup from './CellLookup.vue';
import MapColumns from './MapColumns.vue';
import SheetGrid from './SheetGrid.vue';
import type { SheetHeader, SheetRow } from './sheet';
import { runCheck } from './check';
import type { Check } from './check';
import { msg } from './data';
import type { CreateData } from './data';
import { apply, complete, emptyMapping } from './mapping';
import type { Target } from './mapping';
import { readFile } from './readers';
import type { Sheet as FileSheet } from './readers';
import { column, joinValues, rowRef, rowsOf, splitValues, textOf } from './text';

const props = defineProps<{
	data: CreateData;
	api: Api;
	form: HTMLFormElement;
}>();

const t = ( key: string, ...params: string[] ): string => msg( props.data, key, ...params );

type Field = HTMLTextAreaElement | HTMLInputElement | null;
const field = ( name: string ): Field => props.form.elements.namedItem( name ) as Field;
const area = field( 'rows' ) as HTMLTextAreaElement;
const file = field( 'file' ) as HTMLInputElement | null;
// The text box's field, label and help together, which the grid stands in for.
const areaField = area.closest<HTMLElement>( '.cdx-field' ) ?? area;

const starting = rowsOf( area.value );
// One list of rows, the header first, every row as wide as the header.
const table = ref<string[][]>( starting.length ?
	starting :
	[ [ 'label', 'description', 'aliases' ], [ '', '', '' ] ] );
const mode = ref<'grid' | 'text'>( 'grid' );
const result = ref<Check | null>( null );
const stale = ref( false );
const checking = ref( false );
const problem = ref( '' );
const notice = ref( '' );
const onlyProblems = ref( false );
const panel = ref<'add' | number | null>( null );
const find = ref( '' );
const replacement = ref( '' );
const active = ref<{ r: number; c: number } | null>( null );
const mapping = ref<FileSheet | null>( null );
const adding = reactive( { ...emptyMapping( props.data.lang ), target: 'property' as Target } );

const isTerm = ( x: Target ): boolean => x === 'label' || x === 'description' || x === 'aliases';
const needsProperty = ( x: Target ): boolean => x === 'property' || x === 'reference' || x === 'reads';
const nameLabel = computed( () => t( adding.target === 'new' ? 'ts-sheet-new-label' : 'ts-sheet-plain-name' ) );

const addable = computed<Target[]>( () => {
	const offered: Target[] = [ 'property', 'label', 'description', 'aliases', 'reads', 'reference', 'id' ];
	if ( props.data.canCreateProperty ) {
		offered.splice( 1, 0, 'new' );
	}
	return offered;
} );

function normalize(): void {
	const width = table.value[ 0 ].length;
	for ( const r of table.value ) {
		while ( r.length < width ) {
			r.push( '' );
		}
		r.length = width;
	}
}

// Writes the table into the form's text box: what the form posts.
function sync(): void {
	normalize();
	area.value = textOf( table.value, false );
	stale.value = result.value !== null;
}

const columns = computed( () => table.value[ 0 ].map( ( h ) => column( h, props.data.lang ) ) );
const fresh = computed( () => ( result.value && !stale.value ? result.value : null ) );
const outcome = computed( () => fresh.value?.confirm || fresh.value?.summary || '' );

const headers = computed<SheetHeader[]>( () => columns.value.map( ( c, i ) => {
	const info = result.value?.columns?.[ i ];
	switch ( c.kind ) {
		case 'id':
			return { label: t( 'ts-sheet-target-id' ), sub: 'id', menu: true };
		case 'label':
		case 'description':
		case 'aliases':
			return { label: t( `ts-sheet-target-${ c.kind }` ), sub: c.language, menu: true };
		case 'property':
			return {
				label: info?.label ?? c.property ?? '',
				sub: [ c.property, info?.datatype ].filter( Boolean ).join( ' · ' ),
				menu: true
			};
		case 'new':
			return { label: c.label ?? '', sub: `${ t( 'ts-sheet-new' ) }${ c.datatype ? ` · ${ c.datatype }` : '' }`, menu: true };
		case 'reference':
			return { label: t( 'ts-sheet-reference-heading', c.of ?? '', info?.label ?? c.property ?? '' ), sub: table.value[ 0 ][ i ], menu: true };
		default:
			return { label: c.label ?? '', sub: t( 'ts-sheet-plain' ), menu: true };
	}
} ) );

// What is shown under a cell that names items: their labels, or the row it points at.
function noteOf( cell: string, c: number ): string {
	const info = result.value?.columns?.[ c ];
	const kind = columns.value[ c ].kind;
	if ( cell !== '' && info?.datatype === 'quantity' ) {
		// A quantity's unit, by its label.
		return splitValues( cell )
			.map( ( v ) => result.value?.labels?.[ ( v.split( /\s+/ )[ 1 ] ?? '' ).toUpperCase() ] ?? '' )
			.filter( ( x ) => x !== '' ).join( '; ' );
	}
	if ( !( kind === 'id' || info?.datatype === 'wikibase-item' ) || cell === '' ) {
		return '';
	}
	const labelAt = columns.value.findIndex( ( x ) => x.kind === 'label' );
	return splitValues( cell ).map( ( v ) => {
		const n = rowRef( v );
		if ( n !== null ) {
			const row = table.value[ n ];
			return row ? t( 'ts-sheet-row-named', String( n ), labelAt >= 0 ? row[ labelAt ] : '' ) : '';
		}
		return result.value?.labels?.[ v.toUpperCase() ] ?? '';
	} ).filter( ( x ) => x !== '' ).join( '; ' );
}

const all = computed<SheetRow[]>( () => table.value.slice( 1 ).map( ( cells, i ) => {
	const found = fresh.value?.rows?.[ i ];
	return {
		key: String( i + 1 ),
		lead: String( i + 1 ),
		status: found?.status,
		kind: found?.kind,
		problems: found?.errors,
		removable: true,
		cells,
		notes: cells.map( ( cell, c ) => noteOf( cell, c ) )
	};
} ) );

const troubled = ( kind: string | undefined ): boolean => kind === 'invalid' || kind === 'ambiguous' || kind === 'conflict';
const shown = computed( () => ( onlyProblems.value && fresh.value ?
	all.value.filter( ( r ) => troubled( r.kind ) ) :
	all.value ) );

function edit( key: string, c: number, value: string ): void {
	table.value[ Number( key ) ][ c ] = value;
	sync();
}

function addRow(): void {
	table.value.push( table.value[ 0 ].map( () => '' ) );
	sync();
}

function removeRow( key: string ): void {
	table.value.splice( Number( key ), 1 );
	if ( table.value.length === 1 ) {
		table.value.push( table.value[ 0 ].map( () => '' ) );
	}
	sync();
}

function paste( key: string, c: number, cells: string[][] ): void {
	const width = table.value[ 0 ].length;
	cells.forEach( ( line, dr ) => {
		const r = Number( key ) + dr;
		while ( table.value.length <= r ) {
			table.value.push( table.value[ 0 ].map( () => '' ) );
		}
		line.forEach( ( v, dc ) => {
			if ( c + dc < width ) {
				table.value[ r ][ c + dc ] = v.trim();
			}
		} );
	} );
	sync();
}

function fillFromAbove( key: string, c: number ): void {
	const r = Number( key );
	if ( r > 1 ) {
		table.value[ r ][ c ] = table.value[ r - 1 ][ c ];
		sync();
	}
}

// Copies the active cell of a column, or its first cell, into every cell below it.
function fillDown( c: number ): void {
	const from = active.value && active.value.c === c ? active.value.r : 1;
	const value = table.value[ from ]?.[ c ] ?? '';
	for ( let r = from + 1; r < table.value.length; r++ ) {
		table.value[ r ][ c ] = value;
	}
	sync();
}

function replaceIn( c: number ): void {
	for ( let r = 1; r < table.value.length; r++ ) {
		table.value[ r ][ c ] = table.value[ r ][ c ].split( find.value ).join( replacement.value );
	}
	sync();
}

function removeColumn( c: number ): void {
	if ( table.value[ 0 ].length === 1 ) {
		return;
	}
	for ( const r of table.value ) {
		r.splice( c, 1 );
	}
	panel.value = null;
	sync();
}

function addColumn(): void {
	problem.value = '';
	const m = { ...adding };
	if ( !complete( m ) || ( m.target === 'reads' && m.label.trim() === '' ) ) {
		problem.value = t( 'ts-sheet-add-incomplete' );
		return;
	}
	// The mapping of one new, empty source column gives its header, and the line the
	// reference on every statement gains where it reads the column.
	const out = apply( [ m.label ], [], [ m ], props.data.lang );
	const header = out.rows[ 0 ][ 0 ];
	if ( table.value[ 0 ].some( ( h ) => h.toLowerCase() === header.toLowerCase() ) ) {
		problem.value = t( 'ts-sheet-add-exists', header );
		return;
	}
	table.value[ 0 ].push( header );
	addReference( out.reference );
	panel.value = null;
	sync();
}

function addReference( lines: string[] ): void {
	const box = field( 'reference' );
	if ( !box || !lines.length ) {
		return;
	}
	const have = box.value.split( '\n' ).map( ( l ) => l.trim() );
	const more = lines.filter( ( l ) => !have.includes( l ) );
	box.value = [ box.value.trim(), ...more ].filter( ( l ) => l !== '' ).join( '\n' );
}

function focused( key: string, c: number ): void {
	active.value = { r: Number( key ), c };
}

// The cell a search stands beside: the focused one, in a column of items or of
// quantities, whose units are items.
const lookupFor = computed( () => {
	const a = active.value;
	if ( !a || !table.value[ a.r ] ) {
		return null;
	}
	const kind = columns.value[ a.c ]?.kind;
	const datatype = result.value?.columns?.[ a.c ]?.datatype ?? columns.value[ a.c ]?.datatype;
	const heading = headers.value[ a.c ].label;
	if ( datatype === 'quantity' ) {
		const label = t( 'ts-sheet-find-unit', heading, String( a.r ) );
		return { ...a, unit: true, several: false, label };
	}
	if ( kind !== 'id' && datatype !== 'wikibase-item' ) {
		return null;
	}
	return { ...a, unit: false, several: kind !== 'id', label: t( 'ts-sheet-find-item', heading, String( a.r ) ) };
} );

function picked( id: string ): void {
	const at = lookupFor.value;
	if ( !at || id === '' ) {
		return;
	}
	const cell = table.value[ at.r ][ at.c ];
	if ( at.unit ) {
		// The unit follows the amount the cell holds.
		const amount = cell.trim().split( /\s+/ )[ 0 ] ?? '';
		table.value[ at.r ][ at.c ] = `${ amount } ${ id }`.trim();
	} else {
		const have = splitValues( cell ).filter( ( v ) => /^[A-Za-z]{0,3}Q\d+$/.test( v ) || rowRef( v ) !== null );
		// A cell of unresolved text is replaced; one of IDs gains another.
		const more = at.several && have.length;
		table.value[ at.r ][ at.c ] = more ? joinValues( [ ...have, id ] ) : id;
	}
	sync();
}

async function check(): Promise<void> {
	problem.value = '';
	checking.value = true;
	sync();
	try {
		const found = await runCheck( props.form );
		if ( found.text && found.text !== area.value ) {
			// The check wrote into the text: a matched row's id, a proposed data type.
			area.value = found.text;
			table.value = rowsOf( found.text );
			normalize();
		}
		result.value = found;
		stale.value = false;
	} catch ( e ) {
		problem.value = t( 'ts-sheet-check-failed', e instanceof Error ? e.message : String( e ) );
	} finally {
		checking.value = false;
	}
}

function show( next: 'grid' | 'text' ): void {
	if ( next === mode.value ) {
		return;
	}
	if ( next === 'grid' ) {
		const rows = rowsOf( area.value );
		table.value = rows.length ? rows : [ [ 'label', 'description', 'aliases' ], [ '', '', '' ] ];
		normalize();
		stale.value = result.value !== null;
	} else {
		sync();
	}
	mode.value = next;
	areaField.hidden = next === 'grid';
}

// A chosen file is read here and mapped, and never uploaded: the grid holds its rows.
async function chosen(): Promise<void> {
	const upload = file?.files?.[ 0 ];
	if ( !upload || !file ) {
		return;
	}
	problem.value = '';
	notice.value = '';
	try {
		const sheet = await readFile( upload, props.data.lang );
		file.value = '';
		if ( mode.value === 'text' ) {
			show( 'grid' );
		}
		const body = table.value.slice( 1 ).filter( ( r ) => r.some( ( c ) => c !== '' ) );
		const kept = table.value[ 0 ].filter( ( h ) => h.toLowerCase() !== 'id' );
		const known = kept.some( ( h ) => column( h, props.data.lang ).kind !== 'plain' );
		if ( !body.length && known && kept.length === sheet.headers.length && field( 'draft' ) ) {
			// A saved mapping: a draft with columns and no rows, and a file of its shape.
			table.value = [ kept, ...sheet.rows ];
			notice.value = t( 'ts-sheet-map-saved', String( sheet.rows.length ) );
			sync();
			return;
		}
		mapping.value = sheet;
	} catch ( e ) {
		file.value = '';
		problem.value = t( 'ts-sheet-file-failed', e instanceof Error ? e.message : String( e ) );
	}
}

function mapped( rows: string[][], reference: string[] ): void {
	mapping.value = null;
	table.value = rows.length > 1 ? rows : [ rows[ 0 ], rows[ 0 ].map( () => '' ) ];
	addReference( reference );
	result.value = null;
	sync();
}

const onChosen = (): void => {
	chosen().catch( () => {} );
};

onMounted( () => {
	normalize();
	areaField.hidden = true;
	file?.addEventListener( 'change', onChosen );
	// A page that came back from the server's own check shows its findings already.
	if ( area.value.trim() !== '' && props.form.ownerDocument.querySelector( '.ts-createentities__preview' ) ) {
		check().catch( () => {} );
	}
} );

onBeforeUnmount( () => {
	file?.removeEventListener( 'change', onChosen );
	areaField.hidden = false;
} );
</script>
