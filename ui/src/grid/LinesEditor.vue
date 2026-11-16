<template>
	<fieldset class="ts-sheet-lines">
		<legend class="ts-sheet-lines__legend">
			{{ legend }}
		</legend>
		<p v-if="!lines.length" class="ts-sheet-lines__none">
			{{ t( `ts-sheet-lines-none-${ kind }` ) }}
		</p>
		<ul v-else class="ts-sheet-lines__list">
			<li
				v-for="( line, i ) in lines"
				:key="line.key"
				class="ts-sheet-lines__line">
				<EntityLookup
					:api="api"
					:lang="data.lang"
					type="property"
					:label="t( 'ts-sheet-lines-property', String( i + 1 ) )"
					:placeholder="t( 'ts-sheet-property' )"
					:initial-id="line.property"
					:initial-label="line.propertyLabel"
					@pick="( id, label ) => pickProperty( line, id, label )"
				/>
				<select
					v-if="kind === 'reference' && ( columns.length || line.column )"
					v-model="line.column"
					class="cdx-select"
					:aria-label="t( 'ts-sheet-lines-source', String( i + 1 ) )"
					@change="write">
					<option value="">
						{{ t( 'ts-sheet-lines-fixed' ) }}
					</option>
					<option
						v-for="c in offered( line )"
						:key="c"
						:value="c">
						{{ t( 'ts-sheet-lines-from-column', c ) }}
					</option>
				</select>
				<template v-if="!line.column">
					<EntityLookup
						v-if="line.datatype === 'wikibase-item'"
						:key="`${ line.key }-${ line.property }`"
						:api="api"
						:lang="data.lang"
						type="item"
						:label="t( 'ts-sheet-lines-value', String( i + 1 ) )"
						:placeholder="t( 'ts-sheet-lines-value-item' )"
						:initial-id="line.value"
						:initial-label="line.valueLabel"
						@pick="( id, label ) => pickValue( line, id, label )"
					/>
					<input
						v-else
						v-model="line.value"
						class="cdx-text-input__input ts-sheet-lines__value"
						type="text"
						:aria-label="t( 'ts-sheet-lines-value', String( i + 1 ) )"
						:placeholder="hint( line )"
						@input="write"
						@keydown.enter.prevent>
				</template>
				<span v-if="line.datatype" class="ts-sheet-lines__type">{{ line.datatype }}</span>
				<button
					type="button"
					class="ts-sheet__remove ts-sheet-lines__remove"
					:aria-label="t( 'ts-sheet-lines-remove', String( i + 1 ) )"
					:title="t( 'ts-sheet-lines-remove', String( i + 1 ) )"
					@click="remove( i )">
					×
				</button>
			</li>
		</ul>
		<CdxButton type="button" @click="add">
			<CdxIcon :icon="cdxIconAdd" size="small" />
			{{ t( `ts-sheet-lines-add-${ kind }` ) }}
		</CdxButton>
	</fieldset>
</template>

<script setup lang="ts">
// The statements on every item, or the reference on every statement, of
// Special:CreateManyEntities, as a list of a property and its value over the text box
// the form posts (ADR 0106 §6, A9; 21 §6.7): each line `P31:Q5`, a reference's value
// either given or read from a plain column of the table, `P854:{source url}`. Every
// change is written into the text box, which this list stands in for; a line that names
// no property or no value yet is not written.
import { onBeforeUnmount, onMounted, ref } from 'vue';
import { CdxButton, CdxIcon } from '@wikimedia/codex';
import { cdxIconAdd } from '@wikimedia/codex-icons';
import EntityLookup from '../edit/EntityLookup.vue';
import type { Api } from '../edit/api';
import { msg } from './data';
import type { CreateData } from './data';
import { linesOf, textOfLines } from './lines';
import type { Line as Parsed } from './lines';

const props = defineProps<{
	data: CreateData;
	api: Api;
	/** The text box the list is written into. */
	box: HTMLTextAreaElement;
	kind: 'shared' | 'reference';
	/** The table's plain columns, which a reference's value can be read from. */
	columns: string[];
}>();

const t = ( key: string, ...params: string[] ): string => msg( props.data, key, ...params );

/** A line as it is edited. */
interface Line extends Parsed {
	key: number;
	propertyLabel: string;
	valueLabel: string;
	datatype: string;
}

let keys = 0;
const blank = ( from: Parsed ): Line => ( { ...from, key: keys++, propertyLabel: '', valueLabel: '', datatype: '' } );
const lines = ref<Line[]>( linesOf( props.box.value ).map( blank ) );

// The text box's field, label and help together, which the list stands in for.
const boxField = props.box.closest<HTMLElement>( '.cdx-field' ) ?? props.box;
const legend = boxField.querySelector( 'label' )?.textContent?.trim() ?? '';

// The columns a line can read: the table's plain ones, and the one it reads now.
const offered = ( line: Line ): string[] => (
	line.column && !props.columns.includes( line.column ) ?
		[ ...props.columns, line.column ] :
		props.columns
);

const HINTS: Record<string, string> = {
	time: '1967-01-17',
	quantity: '12 Q11573',
	monolingualtext: 'text@en',
	'globe-coordinate': '52.5, 13.4',
	url: 'https://example.org/'
};
const hint = ( line: Line ): string => HINTS[ line.datatype ] ?? t( 'ts-sheet-lines-value-plain' );

// The page's own text box, which is written to as the form's field, not as a prop.
const box: HTMLTextAreaElement = props.box;

function write(): void {
	box.value = textOfLines( lines.value );
	box.dispatchEvent( new Event( 'input', { bubbles: true } ) );
}

/** An entity as `wbgetentities` answers it. */
interface Found {
	datatype?: string;
	labels?: Record<string, { value: string }>;
}

// The labels and data types of the entities the lines name, in one call.
async function describe( ids: string[] ): Promise<Record<string, Found>> {
	const wanted = [ ...new Set( ids.filter( ( id ) => /^[A-Za-z]{0,3}[PQ]\d+$/.test( id ) ) ) ];
	if ( !wanted.length ) {
		return {};
	}
	try {
		const body = await props.api.get( {
			action: 'wbgetentities',
			ids: wanted.join( '|' ),
			props: 'labels|datatype',
			languages: props.data.uiLang ?? props.data.lang,
			languagefallback: 1
		} );
		return ( body.entities ?? {} ) as Record<string, Found>;
	} catch {
		return {};
	}
}

const labelOf = ( e: Found | undefined ): string => Object.values( e?.labels ?? {} )[ 0 ]?.value ?? '';

async function pickProperty( line: Line, id: string, label: string ): Promise<void> {
	line.property = id.toUpperCase();
	line.propertyLabel = label;
	line.datatype = '';
	write();
	if ( id !== '' ) {
		const found = await describe( [ line.property ] );
		line.datatype = found[ line.property ]?.datatype ?? '';
	}
}

function pickValue( line: Line, id: string, label: string ): void {
	line.value = id;
	line.valueLabel = label;
	write();
}

function add(): void {
	lines.value.push( blank( { property: '', value: '', column: '' } ) );
}

function remove( i: number ): void {
	lines.value.splice( i, 1 );
	write();
}

onMounted( async () => {
	boxField.hidden = true;
	const found = await describe( lines.value.flatMap( ( l ) => [ l.property, l.value ] ) );
	for ( const line of lines.value ) {
		line.propertyLabel = labelOf( found[ line.property ] );
		line.datatype = found[ line.property ]?.datatype ?? '';
		line.valueLabel = labelOf( found[ line.value.toUpperCase() ] );
	}
	// The lookups read their labels when they are made.
	lines.value = lines.value.map( ( l ) => ( { ...l, key: keys++ } ) );
} );

onBeforeUnmount( () => {
	boxField.hidden = false;
} );
</script>
